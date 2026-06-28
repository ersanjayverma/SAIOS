use crate::graphics::{Color, Rect};

pub const MAX_WINDOWS: usize = 16;

#[derive(Debug, Copy, Clone)]
pub struct WindowSurface {
    pub clear_color: Color,
}

#[derive(Debug, Copy, Clone)]
pub struct Window {
	pub id: u32,
	pub title: &'static str,
	pub frame: Rect,
	pub surface: WindowSurface,
	pub border: Color,
	pub visible: bool,
	pub z: u32,
}

impl Window {
	pub const EMPTY: Self = Self {
		id: 0,
		title: "",
		frame: Rect {
			x: 0,
			y: 0,
			width: 0,
			height: 0,
		},
		surface: WindowSurface {
			clear_color: Color::rgb(0, 0, 0),
		},
		border: Color::rgb(0, 0, 0),
		visible: false,
		z: 0,
	};
}

pub struct WindowManager {
	windows: [Window; MAX_WINDOWS],
	len: usize,
	next_id: u32,
}

impl WindowManager {
	pub const fn new() -> Self {
		Self {
			windows: [Window::EMPTY; MAX_WINDOWS],
			len: 0,
			next_id: 1,
		}
	}

	pub fn create_window(
		&mut self,
		title: &'static str,
		frame: Rect,
		bg: Color,
		border: Color,
	) -> Option<u32> {
		if self.len >= MAX_WINDOWS {
			return None;
		}

		let id = self.next_id;
		self.next_id = self.next_id.saturating_add(1);
		self.windows[self.len] = Window {
			id,
			title,
			frame,
			surface: WindowSurface { clear_color: bg },
			border,
			visible: true,
			z: self.len as u32,
		};
		self.len += 1;
		Some(id)
	}

	pub fn close_window(&mut self, id: u32) -> bool {
		if let Some(index) = self.find_index(id) {
			for i in index..self.len.saturating_sub(1) {
				self.windows[i] = self.windows[i + 1];
			}
			self.windows[self.len - 1] = Window::EMPTY;
			self.len -= 1;
			true
		} else {
			false
		}
	}

	pub fn move_window(&mut self, id: u32, x: i32, y: i32) -> bool {
		if let Some(index) = self.find_index(id) {
			self.windows[index].frame.x = x;
			self.windows[index].frame.y = y;
			true
		} else {
			false
		}
	}

	pub fn windows_sorted_by_z(&self) -> [Option<Window>; MAX_WINDOWS] {
		let mut out: [Option<Window>; MAX_WINDOWS] = [None; MAX_WINDOWS];
		for i in 0..self.len {
			out[i] = Some(self.windows[i]);
		}

		let mut i = 1;
		while i < self.len {
			let mut j = i;
			while j > 0 {
				let lhs = out[j - 1].unwrap();
				let rhs = out[j].unwrap();
				if lhs.z <= rhs.z {
					break;
				}
				out[j - 1] = Some(rhs);
				out[j] = Some(lhs);
				j -= 1;
			}
			i += 1;
		}

		out
	}

	fn find_index(&self, id: u32) -> Option<usize> {
		let mut i = 0;
		while i < self.len {
			if self.windows[i].id == id {
				return Some(i);
			}
			i += 1;
		}
		None
	}
}
