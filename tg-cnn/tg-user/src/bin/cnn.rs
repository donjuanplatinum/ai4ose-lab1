#![no_std]
#![no_main]

extern crate alloc;

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
    fc2: Linear<B>,
    relu: Relu,
}

impl<B: Backend> Model<B> {
    pub fn new(device: &B::Device) -> Self {
        let conv1 = Conv2dConfig::new([1, 8], [3, 3]).init(device);
        let conv2 = Conv2dConfig::new([8, 16], [3, 3]).init(device);
        let pool = AdaptiveAvgPool2dConfig::new([7, 7]).init();
        let fc1 = LinearConfig::new(16 * 7 * 7, 128).init(device);
        let fc2 = LinearConfig::new(128, 10).init(device);
        let relu = Relu::new();

        Self {
            conv1,
            conv2,
            pool,
            fc1,
            fc2,
            relu,
        }
    }

    pub fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 2> {
        let x = self.conv1.forward(input);
        let x = self.relu.forward(x);
        let x = self.conv2.forward(x);
        let x = self.relu.forward(x);
        let x = self.pool.forward(x);
        let x = x.flatten(1, 3);
        let x = self.fc1.forward(x);
        let x = self.relu.forward(x);
        self.fc2.forward(x)
    }
}

fn load_mnist_data() -> (Vec<f32>, Vec<u8>) {
    // Simplified: in a real OS we would iterate over mnist_png/training
    // For now, we'll try to read a few files to demonstrate the capability
    println!("Loading MNIST data...");
    let mut images = Vec::new();
    let mut labels = Vec::new();

    // Example: read one image from "0" category
    // This is just a placeholder logic to show integration with file system
    // In actual use, we would use a proper iterator
    for label in 0..10 {
        let path = alloc::format!("mnist_png/training/{}/", label);
        // We'd need a readdir syscall or similar to get filenames
    }
    
    // Returning dummy data for compilation demonstration if needed, 
    // but ideally we decode PNGs here.
    (images, labels)
}

use burn::tensor::TensorData;
use burn::nn::loss::CrossEntropyLossConfig;


#[no_mangle]
pub fn main() -> i32 {
    println!("CNN Training on MNIST starting...");
    
    // RNG Test
    let mut seed_bytes = [0u8; 8];
    let fd = tg_syscall::open("/dev/random\0", user_lib::OpenFlags::RDONLY);
    println!("fd is {}",fd);
    if fd >= 0 {
        read(fd as usize, &mut seed_bytes);
        println!("Random seed sample: {:?}", seed_bytes);
        close(fd as usize);
    } else {
        println!("Warning: /dev/random not found!");
    }

    // Use seed for reproducibility if needed
    let seed = u64::from_le_bytes(seed_bytes);

    type MyBackend = NdArray<f32>;
    // Note: Autodiff requires AutodiffBackend wrapper
    // But burn-ndarray might not have AutodiffBackend in no-std if not configured.
    // Actually, burn provides it.
    
    let device = Default::default();
    let mut model = Model::<MyBackend>::new(&device);
    
    // In burn 0.16, optimizers are often in separate crates or submodules
    // For now, let's just do a forward pass to verify no-std burn works.
    // println!("Model and Optimizer initialized.");
    println!("Model initialized.");
    
    // Training Loop (Manual)
    println!("Training loop started...");
    
    let batch_size = 40;
    let epochs = 20;
    for epoch in 1..=epochs {
        // 1. 生成 64 张输入图片
        let input = Tensor::<MyBackend, 4>::random([batch_size, 1, 28, 28], burn::tensor::Distribution::Default, &device);
        
        // 💡 修复点：动态生成 64 个标签，防止内存越界
        let mut target_vec = Vec::with_capacity(batch_size);
        for i in 0..batch_size {
            target_vec.push((i % 10) as i32); // 循环填入 0-9 的假标签
        }

        // 2. 将这 64 个标签喂给 Tensor
        let targets = Tensor::<MyBackend, 1, Int>::from_data(
            TensorData::from(target_vec.as_slice()),
            &device
        );

        let output = model.forward(input);
        
        let loss_config = CrossEntropyLossConfig::new();
        let loss = loss_config.init(&device).forward(output, targets.clone());

        println!("Epoch {}/{}: Loss={}", epoch, epochs, loss.clone().into_scalar());
    }

    println!("Training completed!");
    0
}
