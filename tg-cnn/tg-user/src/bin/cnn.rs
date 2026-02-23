#![no_std]
#![no_main]

extern crate alloc;
use log::log;

use alloc::vec::Vec;
use burn::backend::NdArray;
use burn::nn::conv::{Conv2d, Conv2dConfig};
use burn::nn::pool::{AdaptiveAvgPool2d, AdaptiveAvgPool2dConfig};
use burn::nn::{Linear, LinearConfig, Relu};
use burn::prelude::*;
use burn::tensor::backend::Backend;
use user_lib::*;

#[derive(Module, Debug)]
pub struct Model<B: Backend> {
    conv1: Conv2d<B>,
    conv2: Conv2d<B>,
    pool: AdaptiveAvgPool2d,
    fc1: Linear<B>,
    pub fc2_weight: Tensor<B, 2>,
    pub fc2_bias: Tensor<B, 1>,
    relu: Relu,
}

impl<B: Backend> Model<B> {
    pub fn new(device: &B::Device) -> Self {
        let conv1 = Conv2dConfig::new([1, 8], [3, 3]).init(device);
        let conv2 = Conv2dConfig::new([8, 16], [3, 3]).init(device);
        let pool = AdaptiveAvgPool2dConfig::new([7, 7]).init();
        let fc1 = LinearConfig::new(16 * 7 * 7, 128).init(device);
        
        // Manual weights for the last layer to allow manual optimization
        let fc2_weight = Tensor::<B, 2>::random([128, 10], burn::tensor::Distribution::Default, device);
        let fc2_bias = Tensor::<B, 1>::zeros([10], device);
        
        let relu = Relu::new();

        Self {
            conv1,
            conv2,
            pool,
            fc1,
            fc2_weight,
            fc2_bias,
            relu,
        }
    }

    pub fn forward(&self, input: Tensor<B, 4>) -> (Tensor<B, 2>, Tensor<B, 2>) {
        let x = self.conv1.forward(input);
        let x = self.relu.forward(x);
        let x = self.conv2.forward(x);
        let x = self.relu.forward(x);
        let x = self.pool.forward(x);
        let x = x.flatten(1, 3);
        let x = self.fc1.forward(x);
        let features = self.relu.forward(x);
        
        // Manual Linear layer: y = x * W + b
        // Output shape: [batch_size, 10]
        // Explicitly unsqueeze bias for broadcasting [10] -> [1, 10]
        let output = features.clone().matmul(self.fc2_weight.clone()).add(self.fc2_bias.clone().unsqueeze_dim(0));
        (output, features)
    }
}

fn load_mnist_data(num_images: usize) -> (Vec<f32>, Vec<u8>) {
    println!("Loading MNIST data ({} images)...", num_images);
    let mut images = Vec::with_capacity(num_images * 28 * 28);
    let mut labels = Vec::with_capacity(num_images);

    // Load images
    let fd_img = open("train-images-subset-ubyte\0", OpenFlags::RDONLY);
    if fd_img < 0 {
        panic!("Failed to open train-images-subset-ubyte");
    }

    let mut img_header = [0u8; 16];
    read(fd_img as usize, &mut img_header);

    let mut img_buf = Vec::with_capacity(num_images * 28 * 28);
    img_buf.resize(num_images * 28 * 28, 0u8);
    read(fd_img as usize, &mut img_buf);

    for b in img_buf {
        images.push(b as f32 / 255.0);
    }
    close(fd_img as usize);

    // Load labels
    let fd_lbl = open("train-labels-subset-ubyte\0", OpenFlags::RDONLY);
    if fd_lbl < 0 {
        panic!("Failed to open train-labels-subset-ubyte");
    }

    let mut lbl_header = [0u8; 8];
    read(fd_lbl as usize, &mut lbl_header);

    let mut lbl_buf = Vec::with_capacity(num_images);
    lbl_buf.resize(num_images, 0u8);
    read(fd_lbl as usize, &mut lbl_buf);

    labels.extend(lbl_buf);
    close(fd_lbl as usize);

    (images, labels)
}

use burn::tensor::TensorData;

fn print_progress(current: usize, total: usize, loss: f32) {
    let width = 30;
    let progress = (current as f32 / total as f32 * width as f32) as usize;
    print!("\r[");
    for i in 0..width {
        if i < progress {
            print!("=");
        } else if i == progress {
            print!(">");
        } else {
            print!(" ");
        }
    }
    print!("] {}/{} Loss: {:.4}", current, total, loss);
}

#[no_mangle]
pub fn main() -> i32 {
    println!("CNN Training on MNIST starting (Manual MSE SGD)...");

    type MyBackend = NdArray<f32>;
    let device = Default::default();
    let mut model = Model::<MyBackend>::new(&device);

    println!("Model initialized.");

    let total_images = 2000;
    let (images, labels) = load_mnist_data(total_images);
    
    // Split into train (80%) and test (20%)
    let num_train = 1600;
    let num_test = 400;
    
    let train_images = images[..num_train * 28 * 28].to_vec();
    let train_labels = labels[..num_train].to_vec();
    let test_images = images[num_train * 28 * 28..].to_vec();
    let test_labels = labels[num_train..].to_vec();

    println!("Dataset split: {} training, {} testing", num_train, num_test);
    println!("Training loop started...");

    let batch_size = 32;
    let epochs = 5;
    let learning_rate = 0.01;
    let num_batches = num_train / batch_size;

    for epoch in 1..=epochs {
        println!("Epoch {}/{}", epoch, epochs);
        let mut epoch_loss = 0.0;
        
        // Training phase
        for batch_idx in 0..num_batches {
            let start = batch_idx * batch_size;

            let batch_imgs = &train_images[start * 28 * 28..(start + batch_size) * 28 * 28];
            let input = Tensor::<MyBackend, 4>::from_data(
                TensorData::new(batch_imgs.to_vec(), [batch_size, 1, 28, 28]),
                &device,
            );

            // One-hot encoding for MSE
            let mut one_hot_data = Vec::with_capacity(batch_size * 10);
            for i in 0..batch_size {
                let label = train_labels[start + i];
                for class in 0..10 {
                    one_hot_data.push(if class == label as usize { 1.0 } else { 0.0 });
                }
            }
            let targets = Tensor::<MyBackend, 2>::from_data(
                TensorData::new(one_hot_data, [batch_size, 10]),
                &device
            );

            let (output, features) = model.forward(input);
            
            // Manual MSE Loss: mean((output - targets)^2)
            let loss = output.clone().sub(targets.clone()).powf_scalar(2.0).mean();
            let loss_val = loss.clone().into_scalar();
            epoch_loss += loss_val;

            // Manual SGD for the last layer (fc2)
            // Grad(output) = (output - targets) / batch_size
            let output_grad = output.sub(targets).div_scalar(batch_size as f32);
            
            // Grad(W) = features^T * output_grad
            let weight_grad = features.transpose().matmul(output_grad.clone());
            
            // Grad(b) = sum(output_grad, axis=0)
            let bias_grad = output_grad.sum_dim(0).flatten(0, 1);
            
            // Update weights
            model.fc2_weight = model.fc2_weight.clone().sub(weight_grad.mul_scalar(learning_rate));
            model.fc2_bias = model.fc2_bias.clone().sub(bias_grad.mul_scalar(learning_rate));

            print_progress(batch_idx + 1, num_batches, loss_val);
        }
        println!(); // New line after progress bar

        // Evaluation phase (Accuracy)
        let mut correct = 0;
        let test_batches = num_test / batch_size;
        for batch_idx in 0..test_batches {
            let start = batch_idx * batch_size;
            let batch_imgs = &test_images[start * 28 * 28..(start + batch_size) * 28 * 28];
            let input = Tensor::<MyBackend, 4>::from_data(
                TensorData::new(batch_imgs.to_vec(), [batch_size, 1, 28, 28]),
                &device,
            );

            let (output, _) = model.forward(input);
            let predictions = output.argmax(1).flatten::<1>(0, 1).into_data();
            
            let batch_lbls = &test_labels[start..start + batch_size];
            for (p, &l) in predictions.as_slice::<i64>().unwrap().iter().zip(batch_lbls.iter()) {
                if (*p as u8) == l {
                    correct += 1;
                }
            }
        }
        
        let accuracy = (correct as f32 / (test_batches * batch_size) as f32) * 100.0;
        println!(
            "Epoch {}/{}: Avg Loss={:.4}, Accuracy={:.2}%",
            epoch,
            epochs,
            epoch_loss / num_batches as f32,
            accuracy
        );
    }

    println!("Training completed!");
    0
}
