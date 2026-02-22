#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
#[allow(unused_imports)]
extern crate alloc;

use user_lib::*;

// ── 游戏常量 ────────────────────────────────────────────────
const WIDTH: usize = 800;
const HEIGHT: usize = 600;
const PADDLE_W: usize = 20;
const PADDLE_H: usize = 100;
const BALL_W: usize = 20;
const PAD_SPEED: isize = 8;

// RGBA colors
const C_BG:     u32 = 0x0d1117ff;
const C_PADDLE: u32 = 0xc9d1d9ff;
const C_BALL:   u32 = 0xf78166ff;
const C_LINE:   u32 = 0x30363dff;

// Linux evdev scancodes (same numbers QEMU virtio-keyboard sends)
const KEY_W:    usize = 17;
const KEY_S:    usize = 31;
const KEY_UP:   usize = 103;
const KEY_DOWN: usize = 108;

static mut FB: *mut u32 = core::ptr::null_mut();

// ── 绘图工具 ─────────────────────────────────────────────────
fn fill(color: u32) {
    for i in 0..WIDTH * HEIGHT {
        unsafe { *FB.add(i) = color; }
    }
}

fn rect(x: isize, y: isize, w: usize, h: usize, c: u32) {
    let x0 = x.max(0) as usize;
    let y0 = y.max(0) as usize;
    let x1 = ((x + w as isize) as usize).min(WIDTH);
    let y1 = ((y + h as isize) as usize).min(HEIGHT);
    for row in y0..y1 {
        for col in x0..x1 {
            unsafe { *FB.add(row * WIDTH + col) = c; }
        }
    }
}

fn center_dashes() {
    let cx = WIDTH / 2;
    let mut y = 0usize;
    while y < HEIGHT {
        for r in y..(y + 16).min(HEIGHT) {
            unsafe { *FB.add(r * WIDTH + cx) = C_LINE; }
        }
        y += 32;
    }
}

// ── 入口 ─────────────────────────────────────────────────────
#[no_mangle]
fn main() -> i32 {
    // 申请帧缓冲共享内存
    let fb_id = shmget(1, WIDTH * HEIGHT * 4);
    let fb_ptr = shmat(fb_id as usize) as *mut u32;
    if fb_ptr as isize == -1 {
        println!("FB shmat failed");
        return -1;
    }
    unsafe { FB = fb_ptr; }

    // 初始游戏状态
    let mut p1y = (HEIGHT / 2 - PADDLE_H / 2) as isize;
    let mut p2y = (HEIGHT / 2 - PADDLE_H / 2) as isize;
    let mut bx  = (WIDTH  / 2 - BALL_W  / 2) as isize;
    let mut by  = (HEIGHT / 2 - BALL_W  / 2) as isize;
    let mut bdx: isize = 6;
    let mut bdy: isize = 5;
    let mut s1 = 0u32;
    let mut s2 = 0u32;

    let mut keys = [0u8; 256];

    println!("Ping Pong!  P1=W/S   P2=Up/Down");

    // ── 主循环 ────────────────────────────────────────────────
    loop {
        sleep(16); // ~60 fps

        // ── 1. 读取按键状态 ──────────────────────────────────
        read(3, &mut keys);

        // ── 2. 移动挡板 ──────────────────────────────────────
        if keys[KEY_W] != 0 { p1y = (p1y - PAD_SPEED).max(0); }
        if keys[KEY_S] != 0 { p1y = (p1y + PAD_SPEED).min(HEIGHT as isize - PADDLE_H as isize); }
        if keys[KEY_UP]   != 0 { p2y = (p2y - PAD_SPEED).max(0); }
        if keys[KEY_DOWN] != 0 { p2y = (p2y + PAD_SPEED).min(HEIGHT as isize - PADDLE_H as isize); }

        // ── 3. 移动球 ────────────────────────────────────────
        bx += bdx;
        by += bdy;

        // 上下墙壁
        if by <= 0 { by = 0; bdy = bdy.abs(); }
        if by + BALL_W as isize >= HEIGHT as isize {
            by = HEIGHT as isize - BALL_W as isize;
            bdy = -bdy.abs();
        }

        // 左挡板碰撞
        if bdx < 0
            && bx <= PADDLE_W as isize
            && by + BALL_W as isize >= p1y
            && by <= p1y + PADDLE_H as isize
        {
            bdx = bdx.abs();
            bx = PADDLE_W as isize;
        }

        // 右挡板碰撞
        if bdx > 0
            && bx + BALL_W as isize >= (WIDTH - PADDLE_W) as isize
            && by + BALL_W as isize >= p2y
            && by <= p2y + PADDLE_H as isize
        {
            bdx = -bdx.abs();
            bx = (WIDTH - PADDLE_W - BALL_W) as isize;
        }

        // 失球得分
        if bx <= 0 {
            s2 += 1;
            println!("P1:{} P2:{}", s1, s2);
            bx = (WIDTH / 2) as isize; by = (HEIGHT / 2) as isize;
            bdx = -6; bdy = 5;
        } else if bx + BALL_W as isize >= WIDTH as isize {
            s1 += 1;
            println!("P1:{} P2:{}", s1, s2);
            bx = (WIDTH / 2) as isize; by = (HEIGHT / 2) as isize;
            bdx = 6; bdy = 5;
        }

        // ── 4. 渲染 ──────────────────────────────────────────
        fill(C_BG);
        center_dashes();
        rect(0,                          p1y, PADDLE_W, PADDLE_H, C_PADDLE);
        rect((WIDTH - PADDLE_W) as isize, p2y, PADDLE_W, PADDLE_H, C_PADDLE);
        rect(bx, by, BALL_W, BALL_W, C_BALL);

        write(4, unsafe { core::slice::from_raw_parts(FB as *const u8, WIDTH * HEIGHT * 4) });
    }
}
