#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{read, write, get_time};

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
        // Simple LCG
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

    fn step(&mut self) {
        if self.game_over { return; }

        let head = self.snake[self.head_idx];
        let mut next_head = head;

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

        // Check self collision (ignoring tail if we're not growing, but to be simple, just check all)
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
        // 1. Clear Screen (Black)
        draw_rect(0, 0, WIDTH*TILE_SIZE, HEIGHT*TILE_SIZE, 0);

        // 2. Draw Food (Yellow)
        draw_rect(self.food.x as usize * TILE_SIZE, self.food.y as usize * TILE_SIZE, TILE_SIZE, TILE_SIZE, 0xFFFF00FF);

        // 3. Draw Snake
        let mut i = self.tail_idx;
        while i != self.head_idx {
            let p = self.snake[i];
            draw_rect(p.x as usize * TILE_SIZE, p.y as usize * TILE_SIZE, TILE_SIZE, TILE_SIZE, 0x00FF00FF);
            i = (i + 1) % self.snake.len();
        }
        // Head (Red)
        let h = self.snake[self.head_idx];
        draw_rect(h.x as usize * TILE_SIZE, h.y as usize * TILE_SIZE, TILE_SIZE, TILE_SIZE, 0xFF0000FF);

        // 4. Flush
        flush_screen();
    }
}

fn paint_pixel(x: usize, y: usize, color: u32) {
    draw_rect(x, y, 1, 1, color);
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

#[no_mangle]
pub fn main() -> i32 {
    println!("Starting Snake (Polling Mode)...");
    
    let mut game = SnakeGame::new();
    let mut last_update = get_time();
    let mut frame_count = 0;

    loop {
        // 1. Poll input non-blocking
        let mut ev = InputEvent { sec: 0, usec: 0, type_: 0, code: 0, value: 0 };
        let ptr = &mut ev as *mut _ as *mut u8;
        let buf = unsafe { core::slice::from_raw_parts_mut(ptr, 24) };
        let ret = read(INPUT_FD, buf);

        if ret == 24 {
            if ev.type_ == EV_KEY && ev.value == 1 { // Key press
                match ev.code {
                    KEY_UP if game.direction != Direction::Down => game.direction = Direction::Up,
                    KEY_DOWN if game.direction != Direction::Up => game.direction = Direction::Down,
                    KEY_LEFT if game.direction != Direction::Right => game.direction = Direction::Left,
                    KEY_RIGHT if game.direction != Direction::Left => game.direction = Direction::Right,
                    _ => {}
                }
            }
        }

        // 2. Update logic
        let now = get_time();
        if now - last_update >= 200 { // 5 FPS
            game.step();
            if game.game_over {
                println!("Game Over! Score: {}", game.len);
                break;
            }
            game.render();
            last_update = now;
            frame_count += 1;
            if frame_count % 10 == 0 {
                println!("Snake heartbeat: {} frames", frame_count);
            }
        } else {
            // yield to prevent 100% CPU lock in poll
            user_lib::sched_yield();
        }
    }

    0
}
