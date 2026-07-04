//! Software graphics stack: an off-screen [`surface::Surface`] drawn into by a
//! [`renderer::Renderer`] and blitted to hardware through a
//! [`display::Display`]. [`framebuffer::Color`] and [`font`] provide the shared
//! color and glyph primitives.
pub mod display;
pub mod font;
pub mod framebuffer;
pub mod renderer;
pub mod surface;
