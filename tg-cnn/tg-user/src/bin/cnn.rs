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
    pub conv1_w: Tensor<B, 4>,
    pub conv1_b: Tensor<B, 1>,
    pub conv2_w: Tensor<B, 4>,
    pub conv2_b: Tensor<B, 1>,
    pub fc1_w: Tensor<B, 2>,
    pub fc1_b: Tensor<B, 1>,
    pub fc2_w: Tensor<B, 2>,
    pub fc2_b: Tensor<B, 1>,
}

impl<B: Backend> Model<B> {
    pub fn new(device: &B::Device) -> Self {
        // [修改 1] 改进初始化：使用 Kaiming 正态分布，根据输入连接数 (fan_in) 缩放
        // Conv1: fan_in = 1 * 3 * 3 = 9. std = sqrt(2/fan_in) ≈ 0.47
        let conv1_w = Tensor::random([8, 1, 3, 3], burn::tensor::Distribution::Normal(0.0, 0.47), device);
        let conv1_b = Tensor::zeros([8], device);
        
        // Conv2: fan_in = 8 * 3 * 3 = 72. std = sqrt(2/fan_in) ≈ 0.16
        let conv2_w = Tensor::random([16, 8, 3, 3], burn::tensor::Distribution::Normal(0.0, 0.16), device);
        let conv2_b = Tensor::zeros([16], device);
        
        // FC1: fan_in = 16 * 7 * 7 = 784. std = sqrt(2/fan_in) ≈ 0.05
        let fc1_w = Tensor::random([16 * 7 * 7, 128], burn::tensor::Distribution::Normal(0.0, 0.05), device);
        let fc1_b = Tensor::zeros([128], device);
        
        // FC2: fan_in = 128. std = sqrt(2/fan_in) ≈ 0.125
        let fc2_w = Tensor::random([128, 10], burn::tensor::Distribution::Normal(0.0, 0.125), device);
        let fc2_b = Tensor::zeros([10], device);

        Self {
            conv1_w,
            conv1_b,
            conv2_w,
            conv2_b,
            fc1_w,
            fc1_b,
            fc2_w,
            fc2_b,
        }
    }

    pub fn forward_all(&self, input: Tensor<B, 4>) -> (
        Tensor<B, 2>, // output (logits)
        Tensor<B, 4>, // c1_relu
        Tensor<B, 4>, // c2_relu
        Tensor<B, 4>, // pool
        Tensor<B, 2>, // fc1_relu
        Tensor<B, 4>  // input (saved)
    ) {
        let options = burn::tensor::ops::ConvOptions::new([1, 1], [1, 1], [1, 1], 1);
        
        // Conv1
        let c1 = burn::tensor::module::conv2d(input.clone(), self.conv1_w.clone(), Some(self.conv1_b.clone()), options.clone());
        let c1_relu = burn::nn::Relu::new().forward(c1);

        // Conv2
        let c2 = burn::tensor::module::conv2d(c1_relu.clone(), self.conv2_w.clone(), Some(self.conv2_b.clone()), options);
        let c2_relu = burn::nn::Relu::new().forward(c2);

        // Pool (28x28 -> 7x7 using 4x4 blocks, stride 4 implicitly via adaptive)
        let pool = burn::tensor::module::adaptive_avg_pool2d(c2_relu.clone(), [7, 7]);
        let flattened = pool.clone().flatten(1, 3);

        // FC1
        let fc1 = flattened.matmul(self.fc1_w.clone()).add(self.fc1_b.clone().unsqueeze_dim(0));
        let fc1_relu = burn::nn::Relu::new().forward(fc1);

        // FC2 (logits)
        let fc2 = fc1_relu.clone().matmul(self.fc2_w.clone()).add(self.fc2_b.clone().unsqueeze_dim(0));

        (fc2, c1_relu, c2_relu, pool, fc1_relu, input)
    }
    
    pub fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 2> {
        self.forward_all(input).0
    }
}

fn load_mnist_data(num_images: usize) -> (Vec<f32>, Vec<u8>) {
    println!("Loading MNIST data ({} images)...", num_images);
    let mut images = Vec::with_capacity(num_images * 28 * 28);
    let mut labels = Vec::with_capacity(num_images);

    // Load images
    let fname_img = "train-images-subset-ubyte\0";
    let fd_img = open(fname_img, OpenFlags::RDONLY);
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
    let fname_lbl = "train-labels-subset-ubyte\0";
    let fd_lbl = open(fname_lbl, OpenFlags::RDONLY);
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
    println!("CNN Training on MNIST starting (Full Manual Backprop)...");

    type MyBackend = NdArray<f32>;
    let device = Default::default();
    let mut model = Model::<MyBackend>::new(&device);

    println!("Model initialized.");

    // [修改 2] 提升数据集大小
    let total_images = 2000;
    let (images, labels) = load_mnist_data(total_images);
    
    // Split into train (1600) and test (400)
    let num_train = 1600;
    let num_test = 400;
    
    let train_images = images[..num_train * 28 * 28].to_vec();
    let train_labels = labels[..num_train].to_vec();
    let test_images = images[num_train * 28 * 28..].to_vec();
    let test_labels = labels[num_train..].to_vec();

    println!("Dataset split: {} training, {} testing", num_train, num_test);
    println!("Training loop started...");

    let batch_size = 16;
    let epochs = 20;
    // [修改 3] 学习率微调
    let lr = 0.05; 
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

            let (output, c1_relu, c2_relu, pool, fc1_relu, _) = model.forward_all(input.clone());
            
            // 1. 计算 softmax 概率
            let max_logits = output.clone().max_dim(1).detach();
            let exp_logits = output.clone().sub(max_logits).exp();
            let sum_exp = exp_logits.clone().sum_dim(1);
            let probs = exp_logits.div(sum_exp);

            // 2. 计算 CE Loss
            let log_probs = probs.clone().add_scalar(1e-7).log();
            let loss = targets.clone().mul(log_probs).sum_dim(1).mean().mul_scalar(-1.0);
            let loss_val = loss.into_scalar();
            epoch_loss += loss_val;

            // --- FULL BACKWARD PASS ---
            
            // Softmax + CrossEntropy Gradient: dL/dz = (probs - targets) / batch_size
            let d_output = probs.sub(targets).div_scalar(batch_size as f32);

            // 1. FC2
            let d_fc2_w = fc1_relu.clone().transpose().matmul(d_output.clone());
            let d_fc2_b = d_output.clone().sum_dim(0).reshape([10]);
            let d_fc1_relu_raw = d_output.matmul(model.fc2_w.clone().transpose());

            // 2. FC1
            let mask1 = fc1_relu.clone().lower_equal(Tensor::zeros_like(&fc1_relu));
            let d_fc1 = d_fc1_relu_raw.mask_where(mask1, Tensor::zeros_like(&fc1_relu));
            let flattened = pool.clone().flatten(1, 3);
            let d_fc1_w = flattened.transpose().matmul(d_fc1.clone());
            let d_fc1_b = d_fc1.clone().sum_dim(0).reshape([128]);
            let d_pool_flat = d_fc1.matmul(model.fc1_w.clone().transpose());

            // 3. Pool (Inverse of average pool)
            let d_pool = d_pool_flat.reshape([batch_size, 16, 7, 7]);
            let d_c2_relu = d_pool.repeat_dim(2, 4).repeat_dim(3, 4).div_scalar(16.0);

            // 4. Conv2
            let mask2 = c2_relu.clone().lower_equal(Tensor::zeros_like(&c2_relu));
            let d_c2 = d_c2_relu.mask_where(mask2, Tensor::zeros_like(&c2_relu));
            
            let x2_unfold = burn::tensor::module::unfold4d(
                c1_relu.clone(), 
                [3, 3], 
                burn::tensor::ops::UnfoldOptions::new([1, 1], [1, 1], [1, 1])
            );
            let d_c2_flat = d_c2.clone().flatten(2, 3); 
            let d_conv2_w = d_c2_flat.matmul(x2_unfold.transpose())
                .sum_dim(0)
                .reshape([16, 8, 3, 3]);
            let d_conv2_b = d_c2.clone().sum_dim(0).sum_dim(2).sum_dim(3).reshape([16]);

            // Back to c1_relu
            let d_c1_relu_raw = burn::tensor::module::conv_transpose2d(
                d_c2,
                model.conv2_w.clone(),
                None,
                burn::tensor::ops::ConvTransposeOptions::new([1, 1], [1, 1], [0, 0], [1, 1], 1)
            );

            // 5. Conv1
            let mask1_conv = c1_relu.clone().lower_equal(Tensor::zeros_like(&c1_relu));
            let d_c1 = d_c1_relu_raw.mask_where(mask1_conv, Tensor::zeros_like(&c1_relu));

            let x1_unfold = burn::tensor::module::unfold4d(
                input, 
                [3, 3], 
                burn::tensor::ops::UnfoldOptions::new([1, 1], [1, 1], [1, 1])
            );
            let d_c1_flat = d_c1.clone().flatten(2, 3);
            let d_conv1_w = d_c1_flat.matmul(x1_unfold.transpose())
                .sum_dim(0)
                .reshape([8, 1, 3, 3]);
            let d_conv1_b = d_c1.clone().sum_dim(0).sum_dim(2).sum_dim(3).reshape([8]);
            
            // --- WEIGHT UPDATES (SGD) ---
            model.fc2_w = model.fc2_w.clone().sub(d_fc2_w.mul_scalar(lr));
            model.fc2_b = model.fc2_b.clone().sub(d_fc2_b.mul_scalar(lr));
            model.fc1_w = model.fc1_w.clone().sub(d_fc1_w.mul_scalar(lr));
            model.fc1_b = model.fc1_b.clone().sub(d_fc1_b.mul_scalar(lr));
            model.conv2_w = model.conv2_w.clone().sub(d_conv2_w.mul_scalar(lr));
            model.conv2_b = model.conv2_b.clone().sub(d_conv2_b.mul_scalar(lr));
            model.conv1_w = model.conv1_w.clone().sub(d_conv1_w.mul_scalar(lr));
            model.conv1_b = model.conv1_b.clone().sub(d_conv1_b.mul_scalar(lr));

            print_progress(batch_idx + 1, num_batches, loss_val);
        }
        println!("");

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

            let (output, _, _, _, _, _) = model.forward_all(input);
            let predictions = output.argmax(1).reshape([batch_size]).into_data();
            
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
