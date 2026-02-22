#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{read, write, sleep, sigaction, SignalAction};

const INPUT_FD: usize = 3;
const GPU_FD: usize = 4;
const WIDTH: usize = 40;
const HEIGHT: usize = 30;
const TILE_SIZE: usize = 20;

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq)]
struct Point {
    x: isize,
    y: isize,
}

struct SnakeGame {
    snake: [Point; 256],
    head_idx: usize,
    tail_idx: usize,
    len: usize,
    direction: Direction,
    food: Point,
    game_over: bool,
    rng_state: u64,
}

impl SnakeGame {
    fn new() -> Self {
        let mut game = SnakeGame {
            snake: [Point { x: 0, y: 0 }; 256],
            head_idx: 0,
            tail_idx: 0,
            len: 1,
            direction: Direction::Right,
            food: Point { x: 10, y: 10 },
            game_over: false,
            rng_state: 12345, // simple seed
        };
        game.snake[0] = Point { x: 5, y: 5 };
        game.spawn_food();
        game
    }

    fn rand(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.rng_state
    }

    fn spawn_food(&mut self) {
        loop {
            let x = (self.rand() % (WIDTH as u64)) as isize;
            let y = (self.rand() % (HEIGHT as u64)) as isize;
            
            let mut collision = false;
            let mut i = self.tail_idx;
            while i != self.head_idx {
                if self.snake[i].x == x && self.snake[i].y == y {
                    collision = true;
                    break;
                }
                i = (i + 1) % self.snake.len();
            }
            if self.snake[self.head_idx].x == x && self.snake[self.head_idx].y == y {
                collision = true;
            }

            if !collision {
                self.food = Point { x, y };
                break;
            }
        }
    }

    fn step(&mut self, next_dir: core::ptr::NonNull<Direction>) {
        if self.game_over { return; }

        let head = self.snake[self.head_idx];
        let mut next_head = head;

        let safe_dir = unsafe { core::ptr::read_volatile(next_dir.as_ptr()) };
        self.direction = safe_dir;

        match self.direction {
            Direction::Up => next_head.y -= 1,
            Direction::Down => next_head.y += 1,
            Direction::Left => next_head.x -= 1,
            Direction::Right => next_head.x += 1,
        }

        // Check walls
        if next_head.x < 0 || next_head.x >= WIDTH as isize || next_head.y < 0 || next_head.y >= HEIGHT as isize {
            self.game_over = true;
            return;
        }

        // Check self collision
        let mut i = self.tail_idx;
        while i != self.head_idx {
            if self.snake[i] == next_head {
                self.game_over = true;
                return;
            }
            i = (i + 1) % self.snake.len();
        }

        self.head_idx = (self.head_idx + 1) % self.snake.len();
        self.snake[self.head_idx] = next_head;

        if next_head == self.food {
            self.len += 1;
            self.spawn_food();
        } else {
            self.tail_idx = (self.tail_idx + 1) % self.snake.len();
        }
    }

    fn render(&self) {
        draw_rect(0, 0, WIDTH*TILE_SIZE, HEIGHT*TILE_SIZE, 0);
        draw_rect(self.food.x as usize * TILE_SIZE, self.food.y as usize * TILE_SIZE, TILE_SIZE, TILE_SIZE, 0xFFFF00FF);
        
        let mut i = self.tail_idx;
        while i != self.head_idx {
            let p = self.snake[i];
            draw_rect(p.x as usize * TILE_SIZE, p.y as usize * TILE_SIZE, TILE_SIZE, TILE_SIZE, 0x00FF00FF);
            i = (i + 1) % self.snake.len();
        }
        let h = self.snake[self.head_idx];
        draw_rect(h.x as usize * TILE_SIZE, h.y as usize * TILE_SIZE, TILE_SIZE, TILE_SIZE, 0xFF0000FF);
        flush_screen();
    }
}

fn draw_rect(x: usize, y: usize, w: usize, h: usize, color: u32) {
    let mut buf = [0u32; 5];
    buf[0] = x as u32;
    buf[1] = y as u32;
    buf[2] = w as u32;
    buf[3] = h as u32;
    buf[4] = color;
    let ptr = buf.as_ptr() as *const u8;
    write(GPU_FD, unsafe { core::slice::from_raw_parts(ptr, 20) });
}

fn flush_screen() {
    write(GPU_FD, &[]);
}

// Virtio Input constants
const EV_KEY: u16 = 1;
const KEY_UP: u16 = 103;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;
const KEY_DOWN: u16 = 108;

#[repr(C)]
struct InputEvent {
    sec: u64,
    usec: u64,
    type_: u16,
    code: u16,
    value: u32,
}

static mut GLOBAL_DIR: Direction = Direction::Right;

fn input_handler() {
    let mut ev = InputEvent { sec: 0, usec: 0, type_: 0, code: 0, value: 0 };
    let ptr = &mut ev as *mut _ as *mut u8;
    let buf = unsafe { core::slice::from_raw_parts_mut(ptr, 24) };
    let ret = read(INPUT_FD, buf);

    if ret == 24 {
        if ev.type_ == EV_KEY && ev.value == 1 { // Key press
            unsafe {
                match ev.code {
                    KEY_UP if GLOBAL_DIR != Direction::Down => GLOBAL_DIR = Direction::Up,
                    KEY_DOWN if GLOBAL_DIR != Direction::Up => GLOBAL_DIR = Direction::Down,
                    KEY_LEFT if GLOBAL_DIR != Direction::Right => GLOBAL_DIR = Direction::Left,
                    KEY_RIGHT if GLOBAL_DIR != Direction::Left => GLOBAL_DIR = Direction::Right,
                    _ => {}
                }
            }
        }
    }
    // Need to return via sigreturn
    user_lib::sigreturn();
}

#[no_mangle]
pub fn main() -> i32 {
    println!("Starting Snake (Interrupt Mode)...");
    
    // Register signal handler (using SIGUSR1 typically, but signature varies)
    let action = SignalAction {
        handler: input_handler as usize,
        mask: 0,
    };
    sigaction(1.into(), &action, core::ptr::null_mut());

    let mut game = SnakeGame::new();
    let mut frame_count = 0;
    let dir_ptr = core::ptr::NonNull::new(unsafe { &mut GLOBAL_DIR as *mut Direction }).unwrap();

    loop {
        game.step(dir_ptr);
        if game.game_over {
            println!("Game Over! Score: {}", game.len);
            break;
        }
        game.render();
        frame_count += 1;
        if frame_count % 10 == 0 {
            println!("Snake heartbeat (IRQ): {} frames", frame_count);
        }
        sleep(200);
    }
    0
}
