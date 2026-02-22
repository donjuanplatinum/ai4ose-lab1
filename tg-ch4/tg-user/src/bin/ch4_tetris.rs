#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
extern crate alloc;

use user_lib::*;
use virtio_drivers::device::input::InputEvent;

const WIDTH: usize = 800;
const HEIGHT: usize = 600;
const TILE_SIZE: usize = 24;
const GRID_WIDTH: usize = 10;
const GRID_HEIGHT: usize = 20;

// Colors
const COLOR_BACKGROUND: u32 = 0x000000FF;
const COLOR_BORDER: u32 = 0xFFFFFFFF;
const COLOR_TEXT: u32 = 0x00FF00FF;

// Tetromino colors
const COLORS: [u32; 7] = [
    0x00FFFFFF, // Cyan (I)
    0xFFFF00FF, // Yellow (O)
    0xFF00FFFF, // Purple (T)
    0x00FF00FF, // Green (S)
    0xFF0000FF, // Red (Z)
    0x0000FFFF, // Blue (J)
    0xFF7F00FF, // Orange (L)
];

// 7 types of tetrominoes, each has 4 blocks, each block has (x, y) relative to pivot
type Shape = [(i32, i32); 4];

const SHAPES: [Shape; 7] = [
    [(0, 0), (1, 0), (2, 0), (3, 0)], // I
    [(0, 0), (1, 0), (0, 1), (1, 1)], // O
    [(0, 0), (1, 0), (2, 0), (1, 1)], // T
    [(1, 0), (2, 0), (0, 1), (1, 1)], // S
    [(0, 0), (1, 0), (1, 1), (2, 1)], // Z
    [(0, 0), (0, 1), (1, 1), (2, 1)], // J
    [(2, 0), (0, 1), (1, 1), (2, 1)], // L
];

struct TetrisGame {
    grid: [[i32; GRID_WIDTH]; GRID_HEIGHT],
    score: usize,
    game_over: bool,
    current_x: i32,
    current_y: i32,
    current_shape_idx: usize,
    current_rotation: usize,
    next_shape_idx: usize,
    last_move_time: isize,
    move_interval: isize,
}

impl TetrisGame {
    fn new() -> Self {
        let mut game = Self {
            grid: [[-1; GRID_WIDTH]; GRID_HEIGHT],
            score: 0,
            game_over: false,
            current_x: 0,
            current_y: 0,
            current_shape_idx: 0,
            current_rotation: 0,
            next_shape_idx: 0,
            last_move_time: get_time(),
            move_interval: 1000,
        };
        game.spawn();
        game.next_shape_idx = get_time() as usize % 7;
        game
    }

    fn spawn(&mut self) {
        self.current_shape_idx = self.next_shape_idx;
        self.next_shape_idx = (get_time() as usize) % 7;
        self.current_x = (GRID_WIDTH / 2 - 1) as i32;
        self.current_y = 0;
        self.current_rotation = 0;
        if self.check_collision(self.current_x, self.current_y, 0) {
            self.game_over = true;
        }
        self.last_move_time = get_time(); // Reset drop timer on spawn
    }

    fn get_rotated_points(&self, shape_idx: usize, rotation: usize) -> [(i32, i32); 4] {
        let mut points = SHAPES[shape_idx];
        for _ in 0..rotation {
            for p in points.iter_mut() {
                let x = p.0;
                let y = p.1;
                p.0 = -y;
                p.1 = x;
            }
        }
        // Normalize
        let min_x = points.iter().map(|p| p.0).min().unwrap();
        let min_y = points.iter().map(|p| p.1).min().unwrap();
        for p in points.iter_mut() {
            p.0 -= min_x;
            p.1 -= min_y;
        }
        points
    }

    fn check_collision(&self, x: i32, y: i32, rotation: usize) -> bool {
        let points = self.get_rotated_points(self.current_shape_idx, rotation);
        for p in points.iter() {
            let nx = x + p.0;
            let ny = y + p.1;
            if nx < 0 || nx >= GRID_WIDTH as i32 || ny >= GRID_HEIGHT as i32 {
                return true;
            }
            if ny >= 0 && self.grid[ny as usize][nx as usize] != -1 {
                return true;
            }
        }
        false
    }

    fn lock_shape(&mut self) {
        let points = self.get_rotated_points(self.current_shape_idx, self.current_rotation);
        for p in points.iter() {
            let nx = self.current_x + p.0;
            let ny = self.current_y + p.1;
            if ny >= 0 {
                self.grid[ny as usize][nx as usize] = self.current_shape_idx as i32;
            }
        }
        self.clear_lines();
        self.spawn();
    }

    fn clear_lines(&mut self) {
        let mut lines_cleared = 0;
        let mut r = GRID_HEIGHT as i32 - 1;
        while r >= 0 {
            let mut full = true;
            for c in 0..GRID_WIDTH {
                if self.grid[r as usize][c] == -1 {
                    full = false;
                    break;
                }
            }
            if full {
                lines_cleared += 1;
                for row in (1..=r as usize).rev() {
                    self.grid[row] = self.grid[row - 1];
                }
                self.grid[0] = [-1; GRID_WIDTH];
            } else {
                r -= 1;
            }
        }
        if lines_cleared > 0 {
            self.score += [0, 100, 300, 500, 800][lines_cleared];
            self.move_interval = (1000 - (self.score / 500) as isize * 50).max(200);
        }
    }

    fn move_down(&mut self) {
        if !self.check_collision(self.current_x, self.current_y + 1, self.current_rotation) {
            self.current_y += 1;
        } else {
            self.lock_shape();
        }
        self.last_move_time = get_time();
    }

    fn move_left(&mut self) {
        if !self.check_collision(self.current_x - 1, self.current_y, self.current_rotation) {
            self.current_x -= 1;
        }
    }

    fn move_right(&mut self) {
        if !self.check_collision(self.current_x + 1, self.current_y, self.current_rotation) {
            self.current_x += 1;
        }
    }

    fn rotate(&mut self) {
        let next_rotation = (self.current_rotation + 1) % 4;
        if !self.check_collision(self.current_x, self.current_y, next_rotation) {
            self.current_rotation = next_rotation;
        }
    }

    fn hard_drop(&mut self) {
        while !self.check_collision(self.current_x, self.current_y + 1, self.current_rotation) {
            self.current_y += 1;
        }
        self.lock_shape();
    }

    // Since we can't easily position write() with our current syscall, 
    // we should ideally have a draw_rect that takes the target address or similar.
    // In our kernel, fd=4 write takes a buffer and a fixed count. 
    // I should probably have updated fd=4 write to include an offset or position.
    // Let's assume for now it just writes to the current pointer in FB.
    // Wait, the current fd=4 implementation is:
    // core::ptr::copy_nonoverlapping(ptr.as_ptr(), FB_PTR, count);
    // It ALWAYS writes to the start of the FB! That's bad.
    
    // I MUST fix the kernel's fd=4 write to support an offset (maybe using seek or just adding a parameter).
    // Or I can use fd=4 with count = FB_LEN and pass the WHOLE buffer.
}

#[no_mangle]
fn main() -> i32 {
    let mut game = TetrisGame::new();
    println!("Tetris started!");
    
    // Framebuffer is 800x600x4 bytes = 1,920,000 bytes
    let mut fb = alloc::vec![0u32; WIDTH * HEIGHT];

    loop {
        // 1. Poll input
        let mut ev = InputEvent::default();
        while unsafe { read(3, core::slice::from_raw_parts_mut(&mut ev as *mut _ as *mut u8, 24)) } == 24 {
            if ev.event_type == 1 && ev.value == 1 { // Key down
                match ev.code {
                    17 | 103 => game.rotate(),      // W / Up
                    30 | 105 => game.move_left(),   // A / Left
                    32 | 106 => game.move_right(),  // D / Right
                    31 | 108 => game.move_down(),   // S / Down
                    57  => game.hard_drop(),        // Space
                    _ => {}
                }
            }
        }

        // 2. Gravity
        let now = get_time();
        if now - game.last_move_time >= game.move_interval {
            game.move_down();
        }

        if game.game_over {
            println!("Game Over! Score: {}", game.score);
            break;
        }

        // 3. Render to local buffer
        fb.fill(0); // Black background
        
        // Draw border
        let margin_x = (WIDTH - GRID_WIDTH * TILE_SIZE) / 2;
        let margin_y = (HEIGHT - GRID_HEIGHT * TILE_SIZE) / 2;
        
        // Grid background
        for r in 0..GRID_HEIGHT {
            for c in 0..GRID_WIDTH {
                let color = if game.grid[r][c] == -1 { 0x111111FF } else { COLORS[game.grid[r][c] as usize] };
                draw_block(&mut fb, margin_x + c * TILE_SIZE, margin_y + r * TILE_SIZE, color);
            }
        }
        
        // Current piece
        let points = game.get_rotated_points(game.current_shape_idx, game.current_rotation);
        for p in points.iter() {
            let nx = game.current_x + p.0;
            let ny = game.current_y + p.1;
            if ny >= 0 {
                draw_block(&mut fb, margin_x + nx as usize * TILE_SIZE, margin_y + ny as usize * TILE_SIZE, COLORS[game.current_shape_idx]);
            }
        }

        // 4. Flush to kernel
        write(4, unsafe { core::slice::from_raw_parts(fb.as_ptr() as *const u8, WIDTH * HEIGHT * 4) });
        
        sleep(20);
    }
    0
}

fn draw_block(fb: &mut [u32], x: usize, y: usize, color: u32) {
    for i in 1..TILE_SIZE-1 {
        for j in 1..TILE_SIZE-1 {
            let idx = (y + i) * WIDTH + (x + j);
            if idx < fb.len() {
                fb[idx] = color;
            }
        }
    }
}
