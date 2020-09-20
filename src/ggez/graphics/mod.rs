//! The `graphics` module performs the actual drawing of images, text, and other
//! objects with the [`Drawable`](trait.Drawable.html) trait.  It also handles
//! basic loading of images and text.
//!
//! This module also manages graphics state, coordinate systems, etc.
//! The default coordinate system has the origin in the upper-left
//! corner of the screen, with Y increasing downwards.
//!
//! This library differs significantly in performance characteristics from the
//! `LÖVE` library that it is based on. Many operations that are batched by default
//! in love (e.g. drawing primitives like rectangles or circles) are *not* batched
//! in `ggez`, so render loops with a large number of draw calls can be very slow.
//! The primary solution to efficiently rendering a large number of primitives is
//! a [`SpriteBatch`](spritebatch/struct.SpriteBatch.html), which can be orders
//! of magnitude more efficient than individual
//! draw calls.
//!
//! The `pipe` module is auto-generated by `gfx_defines!`.  You shouldn't need to
//! touch it, but alas we can't exclude it from `cargo doc`.

use std::collections::HashMap;
use std::convert::From;
use std::fmt;
use std::path::Path;
use std::u16;

use gfx;
use gfx::texture;
use gfx::Device;
use gfx::Factory;
use gfx_device_gl;
use glutin;

use crate::ggez::conf;
use crate::ggez::conf::WindowMode;
use crate::ggez::context::Context;
use crate::ggez::context::DebugId;
use crate::ggez::GameError;
use crate::ggez::GameResult;

pub(crate) mod canvas;
pub(crate) mod context;
pub(crate) mod drawparam;
pub(crate) mod image;
pub(crate) mod mesh;
pub(crate) mod shader;
pub(crate) mod text;
pub(crate) mod types;

pub use mint;
pub(crate) use nalgebra as na;

pub mod spritebatch;

pub use crate::ggez::graphics::canvas::*;
pub use crate::ggez::graphics::drawparam::*;
pub use crate::ggez::graphics::image::*;
pub use crate::ggez::graphics::mesh::*;
pub use crate::ggez::graphics::shader::*;
pub use crate::ggez::graphics::text::*;
pub use crate::ggez::graphics::types::*;

// This isn't really particularly nice, but it's only used
// in a couple places and it's not very easy to change or configure.
// Since the next major project is "rewrite the graphics engine" I think
// we're fine just leaving it.
//
// It exists basically because gfx-rs is incomplete and we can't *always*
// specify texture formats and such entirely at runtime, which we need to
// do to make sRGB handling work properly.
pub(crate) type BuggoSurfaceFormat = gfx::format::Srgba8;
type ShaderResourceType = [f32; 4];

/// A trait providing methods for working with a particular backend, such as OpenGL,
/// with associated gfx-rs types for that backend.  As a user you probably
/// don't need to touch this unless you want to write a new graphics backend
/// for ggez.  (Trust me, you don't.)
pub trait BackendSpec: fmt::Debug {
    /// gfx resource type
    type Resources: gfx::Resources;
    /// gfx factory type
    type Factory: gfx::Factory<Self::Resources> + Clone;
    /// gfx command buffer type
    type CommandBuffer: gfx::CommandBuffer<Self::Resources>;
    /// gfx device type
    type Device: gfx::Device<Resources = Self::Resources, CommandBuffer = Self::CommandBuffer>;

    /// A helper function to take a RawShaderResourceView and turn it into a typed one based on
    /// the surface type defined in a `BackendSpec`.
    ///
    /// But right now we only allow surfaces that use [f32;4] colors, so we can freely
    /// hardcode this in the `ShaderResourceType` type.
    fn raw_to_typed_shader_resource(
        &self,
        texture_view: gfx::handle::RawShaderResourceView<Self::Resources>,
    ) -> gfx::handle::ShaderResourceView<<Self as BackendSpec>::Resources, ShaderResourceType> {
        // gfx::memory::Typed is UNDOCUMENTED, aiee!
        // However there doesn't seem to be an official way to turn a raw tex/view into a typed
        // one; this API oversight would probably get fixed, except gfx is moving to a new
        // API model.  So, that also fortunately means that undocumented features like this
        // probably won't go away on pre-ll gfx...
        let typed_view: gfx::handle::ShaderResourceView<_, ShaderResourceType> =
            gfx::memory::Typed::new(texture_view);
        typed_view
    }

    /// Helper function that turns a raw to typed texture.
    /// A bit hacky since we can't really specify surface formats as part
    /// of this that well, alas.  There's some functions, like
    /// `gfx::Encoder::update_texture()`, that don't seem to have a `_raw()`
    /// counterpart, so we need this, so we need `BuggoSurfaceFormat` to
    /// keep fixed at compile time what texture format we're actually using.
    /// Oh well!
    fn raw_to_typed_texture(
        &self,
        texture_view: gfx::handle::RawTexture<Self::Resources>,
    ) -> gfx::handle::Texture<
        <Self as BackendSpec>::Resources,
        <BuggoSurfaceFormat as gfx::format::Formatted>::Surface,
    > {
        let typed_view: gfx::handle::Texture<
            _,
            <BuggoSurfaceFormat as gfx::format::Formatted>::Surface,
        > = gfx::memory::Typed::new(texture_view);
        typed_view
    }

    /// Returns the version of the backend, `(major, minor)`.
    ///
    /// So for instance if the backend is using OpenGL version 3.2,
    /// it would return `(3, 2)`.
    fn version_tuple(&self) -> (u8, u8);

    /// Returns the glutin `Api` enum for this backend.
    fn api(&self) -> glutin::Api;

    /// Returns the text of the vertex and fragment shader files
    /// to create default shaders for this backend.
    fn shaders(&self) -> (&'static [u8], &'static [u8]);

    /// Returns a string containing some backend-dependent info.
    fn info(&self, device: &Self::Device) -> String;

    /// Creates the window.
    fn init<'a>(
        &self,
        window_builder: glutin::WindowBuilder,
        gl_builder: glutin::ContextBuilder<'a>,
        events_loop: &glutin::EventsLoop,
        color_format: gfx::format::Format,
        depth_format: gfx::format::Format,
    ) -> Result<
        (
            glutin::WindowedContext,
            Self::Device,
            Self::Factory,
            gfx::handle::RawRenderTargetView<Self::Resources>,
            gfx::handle::RawDepthStencilView<Self::Resources>,
        ),
        glutin::CreationError,
    >;

    /// Create an Encoder for the backend.
    fn encoder(factory: &mut Self::Factory) -> gfx::Encoder<Self::Resources, Self::CommandBuffer>;

    /// Resizes the viewport for the backend. (right now assumes a Glutin window...)
    fn resize_viewport(
        &self,
        color_view: &gfx::handle::RawRenderTargetView<Self::Resources>,
        depth_view: &gfx::handle::RawDepthStencilView<Self::Resources>,
        color_format: gfx::format::Format,
        depth_format: gfx::format::Format,
        window: &glutin::WindowedContext,
    ) -> Option<(
        gfx::handle::RawRenderTargetView<Self::Resources>,
        gfx::handle::RawDepthStencilView<Self::Resources>,
    )>;
}

/// A backend specification for OpenGL.
/// This is different from [`Backend`](../conf/enum.Backend.html)
/// because this needs to be its own struct to implement traits
/// upon, and because there may need to be a layer of translation
/// between what the user asks for in the config, and what the
/// graphics backend code actually gets from the driver.
///
/// You shouldn't normally need to fiddle with this directly
/// but it has to be public because generic types like
/// [`Shader`](type.Shader.html) depend on it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, SmartDefault)]
pub struct GlBackendSpec {
    #[default = 3]
    major: u8,
    #[default = 2]
    minor: u8,
    #[default(glutin::Api::OpenGl)]
    api: glutin::Api,
}

impl From<conf::Backend> for GlBackendSpec {
    fn from(c: conf::Backend) -> Self {
        match c {
            conf::Backend::OpenGL { major, minor } => Self {
                major,
                minor,
                api: glutin::Api::OpenGl,
            },
            conf::Backend::OpenGLES { major, minor } => Self {
                major,
                minor,
                api: glutin::Api::OpenGlEs,
            },
        }
    }
}

impl BackendSpec for GlBackendSpec {
    type Resources = gfx_device_gl::Resources;
    type Factory = gfx_device_gl::Factory;
    type CommandBuffer = gfx_device_gl::CommandBuffer;
    type Device = gfx_device_gl::Device;

    fn version_tuple(&self) -> (u8, u8) {
        (self.major, self.minor)
    }

    fn api(&self) -> glutin::Api {
        self.api
    }

    fn shaders(&self) -> (&'static [u8], &'static [u8]) {
        match self.api {
            glutin::Api::OpenGl => (
                include_bytes!("shader/basic_150.glslv"),
                include_bytes!("shader/basic_150.glslf"),
            ),
            glutin::Api::OpenGlEs => (
                include_bytes!("shader/basic_es300.glslv"),
                include_bytes!("shader/basic_es300.glslf"),
            ),
            a => panic!("Unsupported API: {:?}, should never happen", a),
        }
    }

    fn init<'a>(
        &self,
        window_builder: glutin::WindowBuilder,
        gl_builder: glutin::ContextBuilder<'a>,
        events_loop: &glutin::EventsLoop,
        color_format: gfx::format::Format,
        depth_format: gfx::format::Format,
    ) -> Result<
        (
            glutin::WindowedContext,
            Self::Device,
            Self::Factory,
            gfx::handle::RawRenderTargetView<Self::Resources>,
            gfx::handle::RawDepthStencilView<Self::Resources>,
        ),
        glutin::CreationError,
    > {
        gfx_window_glutin::init_raw(
            window_builder,
            gl_builder,
            events_loop,
            color_format,
            depth_format,
        )
    }

    fn info(&self, device: &Self::Device) -> String {
        let info = device.get_info();
        format!(
            "Driver vendor: {}, renderer {}, version {:?}, shading language {:?}",
            info.platform_name.vendor,
            info.platform_name.renderer,
            info.version,
            info.shading_language
        )
    }

    fn encoder(factory: &mut Self::Factory) -> gfx::Encoder<Self::Resources, Self::CommandBuffer> {
        factory.create_command_buffer().into()
    }

    fn resize_viewport(
        &self,
        color_view: &gfx::handle::RawRenderTargetView<Self::Resources>,
        depth_view: &gfx::handle::RawDepthStencilView<Self::Resources>,
        color_format: gfx::format::Format,
        depth_format: gfx::format::Format,
        window: &glutin::WindowedContext,
    ) -> Option<(
        gfx::handle::RawRenderTargetView<Self::Resources>,
        gfx::handle::RawDepthStencilView<Self::Resources>,
    )> {
        // Basically taken from the definition of
        // gfx_window_glutin::update_views()
        let dim = color_view.get_dimensions();
        assert_eq!(dim, depth_view.get_dimensions());
        if let Some((cv, dv)) =
            gfx_window_glutin::update_views_raw(window, dim, color_format, depth_format)
        {
            Some((cv, dv))
        } else {
            None
        }
    }
}

const QUAD_VERTS: [Vertex; 4] = [
    Vertex {
        pos: [0.0, 0.0],
        uv: [0.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
    },
    Vertex {
        pos: [1.0, 0.0],
        uv: [1.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
    },
    Vertex {
        pos: [1.0, 1.0],
        uv: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
    },
    Vertex {
        pos: [0.0, 1.0],
        uv: [0.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
    },
];

const QUAD_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

gfx_defines! {
    /// Structure containing fundamental vertex data.
    vertex Vertex {
        pos: [f32; 2] = "a_Pos",
        uv: [f32; 2] = "a_Uv",
        color: [f32;4] = "a_VertColor",
    }

    /// Internal structure containing values that are different for each
    /// drawable object.  This is the per-object data that
    /// gets fed into the shaders.
    vertex InstanceProperties {
        // the columns here are for the transform matrix;
        // you can't shove a full matrix into an instance
        // buffer, annoyingly.
        col1: [f32; 4] = "a_TCol1",
        col2: [f32; 4] = "a_TCol2",
        col3: [f32; 4] = "a_TCol3",
        col4: [f32; 4] = "a_TCol4",
        src: [f32; 4] = "a_Src",
        color: [f32; 4] = "a_Color",
    }

    /// Internal structure containing global shader state.
    constant Globals {
        mvp_matrix: [[f32; 4]; 4] = "u_MVP",
    }

    // Internal structure containing graphics pipeline state.
    // This can't be a doc comment though because it somehow
    // breaks the gfx_defines! macro though.  :-(
    pipeline pipe {
        vbuf: gfx::VertexBuffer<Vertex> = (),
        tex: gfx::TextureSampler<[f32; 4]> = "t_Texture",
        globals: gfx::ConstantBuffer<Globals> = "Globals",
        rect_instance_properties: gfx::InstanceBuffer<InstanceProperties> = (),
        // The default values here are overwritten by the
        // pipeline init values in `shader::create_shader()`.
        out: gfx::RawRenderTarget =
          ("Target0",
           gfx::format::Format(gfx::format::SurfaceType::R8_G8_B8_A8, gfx::format::ChannelType::Srgb),
           gfx::state::ColorMask::all(), Some(gfx::preset::blend::ALPHA)
          ),
    }
}

impl fmt::Display for InstanceProperties {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut matrix_vec: Vec<f32> = vec![];
        matrix_vec.extend(&self.col1);
        matrix_vec.extend(&self.col2);
        matrix_vec.extend(&self.col3);
        matrix_vec.extend(&self.col4);
        let matrix = na::Matrix4::from_column_slice(&matrix_vec);
        writeln!(
            f,
            "Src: ({},{}+{},{})",
            self.src[0], self.src[1], self.src[2], self.src[3]
        )?;
        writeln!(f, "Color: {:?}", self.color)?;
        write!(f, "Matrix: {}", matrix)
    }
}

impl Default for InstanceProperties {
    fn default() -> Self {
        InstanceProperties {
            col1: [1.0, 0.0, 0.0, 0.0],
            col2: [0.0, 1.0, 0.0, 0.0],
            col3: [1.0, 0.0, 1.0, 0.0],
            col4: [1.0, 0.0, 0.0, 1.0],
            src: [0.0, 0.0, 1.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}
/// A structure for conveniently storing `Sampler`'s, based off
/// their `SamplerInfo`.
pub(crate) struct SamplerCache<B>
where
    B: BackendSpec,
{
    samplers: HashMap<texture::SamplerInfo, gfx::handle::Sampler<B::Resources>>,
}

impl<B> SamplerCache<B>
where
    B: BackendSpec,
{
    fn new() -> Self {
        SamplerCache {
            samplers: HashMap::new(),
        }
    }

    fn get_or_insert(
        &mut self,
        info: texture::SamplerInfo,
        factory: &mut B::Factory,
    ) -> gfx::handle::Sampler<B::Resources> {
        let sampler = self
            .samplers
            .entry(info)
            .or_insert_with(|| factory.create_sampler(info));
        sampler.clone()
    }
}

impl From<gfx::buffer::CreationError> for GameError {
    fn from(e: gfx::buffer::CreationError) -> Self {
        use gfx::buffer::CreationError;
        match e {
            CreationError::UnsupportedBind(b) => GameError::RenderError(format!(
                "Could not create buffer: Unsupported Bind ({:?})",
                b
            )),
            CreationError::UnsupportedUsage(u) => GameError::RenderError(format!(
                "Could not create buffer: Unsupported Usage ({:?})",
                u
            )),
            CreationError::Other => {
                GameError::RenderError("Could not create buffer: Unknown error".to_owned())
            }
        }
    }
}

// **********************************************************************
// DRAWING
// **********************************************************************

/// Clear the screen to the background color.
pub fn clear(ctx: &mut Context, color: Color) {
    let gfx = &mut ctx.gfx_context;
    let linear_color: types::LinearColor = color.into();
    let c: [f32; 4] = linear_color.into();
    gfx.encoder.clear_raw(&gfx.data.out, c.into());
}

/// Draws the given `Drawable` object to the screen by calling its
/// [`draw()`](trait.Drawable.html#tymethod.draw) method.
pub fn draw<D, T>(ctx: &mut Context, drawable: &D, params: T) -> GameResult
where
    D: Drawable,
    T: Into<DrawParam>,
{
    let params = params.into();
    drawable.draw(ctx, params)
}

/// Tells the graphics system to actually put everything on the screen.
/// Call this at the end of your [`EventHandler`](../event/trait.EventHandler.html)'s
/// [`draw()`](../event/trait.EventHandler.html#tymethod.draw) method.
///
/// Unsets any active canvas.
pub fn present(ctx: &mut Context) -> GameResult<()> {
    let gfx = &mut ctx.gfx_context;
    gfx.data.out = gfx.screen_render_target.clone();
    // We might want to give the user more control over when the
    // encoder gets flushed eventually, if we want them to be able
    // to do their own gfx drawing.  HOWEVER, the whole pipeline type
    // thing is a bigger hurdle, so this is fine for now.
    gfx.encoder.flush(&mut *gfx.device);
    gfx.window.swap_buffers()?;
    gfx.device.cleanup();
    Ok(())
}

/// Take a screenshot by outputting the current render surface
/// (screen or selected canvas) to an `Image`.
pub fn screenshot(ctx: &mut Context) -> GameResult<Image> {
    use gfx::memory::Bind;
    let debug_id = DebugId::get(ctx);

    let gfx = &mut ctx.gfx_context;
    let (w, h, _depth, aa) = gfx.data.out.get_dimensions();
    if aa != gfx_core::texture::AaMode::Single {
        // Details see https://github.com/ggez/ggez/issues/751
        return Err(GameError::RenderError("Can't take screenshots of anti aliased textures.\n(since neither copying or resolving them is supported right now)".to_string()));
    }

    let surface_format = gfx.color_format();
    let gfx::format::Format(surface_type, channel_type) = surface_format;

    let texture_kind = gfx::texture::Kind::D2(w, h, aa);
    let texture_info = gfx::texture::Info {
        kind: texture_kind,
        levels: 1,
        format: surface_type,
        bind: Bind::TRANSFER_SRC | Bind::TRANSFER_DST | Bind::SHADER_RESOURCE,
        usage: gfx::memory::Usage::Data,
    };
    let target_texture = gfx
        .factory
        .create_texture_raw(texture_info, Some(channel_type), None)?;
    let image_info = gfx::texture::ImageInfoCommon {
        xoffset: 0,
        yoffset: 0,
        zoffset: 0,
        width: w,
        height: h,
        depth: 0,
        format: surface_format,
        mipmap: 0,
    };

    let mut local_encoder: gfx::Encoder<gfx_device_gl::Resources, gfx_device_gl::CommandBuffer> =
        gfx.factory.create_command_buffer().into();

    local_encoder.copy_texture_to_texture_raw(
        gfx.data.out.get_texture(),
        None,
        image_info,
        &target_texture,
        None,
        image_info,
    )?;

    local_encoder.flush(&mut *gfx.device);

    let resource_desc = gfx::texture::ResourceDesc {
        channel: channel_type,
        layer: None,
        min: 0,
        max: 0,
        swizzle: gfx::format::Swizzle::new(),
    };
    let shader_resource = gfx
        .factory
        .view_texture_as_shader_resource_raw(&target_texture, resource_desc)?;
    let image = Image {
        texture: shader_resource,
        texture_handle: target_texture,
        sampler_info: gfx.default_sampler_info,
        blend_mode: None,
        width: w,
        height: h,
        debug_id,
    };

    Ok(image)
}

// **********************************************************************
// GRAPHICS STATE
// **********************************************************************

/// Get the default filter mode for new images.
pub fn default_filter(ctx: &Context) -> FilterMode {
    let gfx = &ctx.gfx_context;
    gfx.default_sampler_info.filter.into()
}

/// Returns a string that tells a little about the obtained rendering mode.
/// It is supposed to be human-readable and will change; do not try to parse
/// information out of it!
pub fn renderer_info(ctx: &Context) -> GameResult<String> {
    let backend_info = ctx.gfx_context.backend_spec.info(&*ctx.gfx_context.device);
    Ok(format!(
        "Requested {:?} {}.{} Core profile, actually got {}.",
        ctx.gfx_context.backend_spec.api,
        ctx.gfx_context.backend_spec.major,
        ctx.gfx_context.backend_spec.minor,
        backend_info
    ))
}

/// Returns a rectangle defining the coordinate system of the screen.
/// It will be `Rect { x: left, y: top, w: width, h: height }`
///
/// If the Y axis increases downwards, the `height` of the `Rect`
/// will be negative.
pub fn screen_coordinates(ctx: &Context) -> Rect {
    ctx.gfx_context.screen_rect
}

/// Sets the default filter mode used to scale images.
///
/// This does not apply retroactively to already created images.
pub fn set_default_filter(ctx: &mut Context, mode: FilterMode) {
    let gfx = &mut ctx.gfx_context;
    let new_mode = mode.into();
    let sampler_info = texture::SamplerInfo::new(new_mode, texture::WrapMode::Clamp);
    // We create the sampler now so we don't end up creating it at some
    // random-ass time while we're trying to draw stuff.
    let _sampler = gfx.samplers.get_or_insert(sampler_info, &mut *gfx.factory);
    gfx.default_sampler_info = sampler_info;
}

/// Sets the bounds of the screen viewport.
///
/// The default coordinate system has (0,0) at the top-left corner
/// with X increasing to the right and Y increasing down, with the
/// viewport scaled such that one coordinate unit is one pixel on the
/// screen.  This function lets you change this coordinate system to
/// be whatever you prefer.
///
/// The `Rect`'s x and y will define the top-left corner of the screen,
/// and that plus its w and h will define the bottom-right corner.
pub fn set_screen_coordinates(context: &mut Context, rect: Rect) -> GameResult {
    let gfx = &mut context.gfx_context;
    gfx.set_projection_rect(rect);
    gfx.calculate_transform_matrix();
    gfx.update_globals()
}

/// Sets the raw projection matrix to the given homogeneous
/// transformation matrix.
///
/// You must call [`apply_transformations(ctx)`](fn.apply_transformations.html)
/// after calling this to apply these changes and recalculate the
/// underlying MVP matrix.
pub fn set_projection<M>(context: &mut Context, proj: M)
where
    M: Into<mint::ColumnMatrix4<f32>>,
{
    let proj = Matrix4::from(proj.into());
    let gfx = &mut context.gfx_context;
    gfx.set_projection(proj);
}

/// Premultiplies the given transformation matrix with the current projection matrix
///
/// You must call [`apply_transformations(ctx)`](fn.apply_transformations.html)
/// after calling this to apply these changes and recalculate the
/// underlying MVP matrix.
pub fn mul_projection<M>(context: &mut Context, transform: M)
where
    M: Into<mint::ColumnMatrix4<f32>>,
{
    let transform = Matrix4::from(transform.into());
    let gfx = &mut context.gfx_context;
    let curr = gfx.projection();
    gfx.set_projection(transform * curr);
}

/// Gets a copy of the context's raw projection matrix
pub fn projection(context: &Context) -> mint::ColumnMatrix4<f32> {
    let gfx = &context.gfx_context;
    gfx.projection().into()
}

/// Pushes a homogeneous transform matrix to the top of the transform
/// (model) matrix stack of the `Context`. If no matrix is given, then
/// pushes a copy of the current transform matrix to the top of the stack.
///
/// You must call [`apply_transformations(ctx)`](fn.apply_transformations.html)
/// after calling this to apply these changes and recalculate the
/// underlying MVP matrix.
///
/// A [`DrawParam`](struct.DrawParam.html) can be converted into an appropriate
/// transform matrix by turning it into a [`DrawTransform`](struct.DrawTransform.html):
///
/// ```rust,no_run
/// # use ggez::*;
/// # use ggez::graphics::*;
/// # fn main() {
/// # let ctx = &mut ContextBuilder::new("foo", "bar").build().unwrap().0;
/// let param = /* DrawParam building */
/// #   DrawParam::new();
/// let transform = param.to_matrix();
/// graphics::push_transform(ctx, Some(transform));
/// # }
/// ```
pub fn push_transform<M>(context: &mut Context, transform: Option<M>)
where
    M: Into<mint::ColumnMatrix4<f32>>,
{
    let transform = transform.map(|transform| Matrix4::from(transform.into()));
    let gfx = &mut context.gfx_context;
    if let Some(t) = transform {
        gfx.push_transform(t);
    } else {
        let copy = *gfx
            .modelview_stack
            .last()
            .expect("Matrix stack empty, should never happen");
        gfx.push_transform(copy);
    }
}

/// Pops the transform matrix off the top of the transform
/// (model) matrix stack of the `Context`.
///
/// You must call [`apply_transformations(ctx)`](fn.apply_transformations.html)
/// after calling this to apply these changes and recalculate the
/// underlying MVP matrix.
pub fn pop_transform(context: &mut Context) {
    let gfx = &mut context.gfx_context;
    gfx.pop_transform();
}

/// Sets the current model transformation to the given homogeneous
/// transformation matrix.
///
/// You must call [`apply_transformations(ctx)`](fn.apply_transformations.html)
/// after calling this to apply these changes and recalculate the
/// underlying MVP matrix.
///
/// A [`DrawParam`](struct.DrawParam.html) can be converted into an appropriate
/// transform matrix with `DrawParam::to_matrix()`.
/// ```rust,no_run
/// # use ggez::*;
/// # use ggez::graphics::*;
/// # fn main() {
/// # let ctx = &mut ContextBuilder::new("foo", "bar").build().unwrap().0;
/// let param = /* DrawParam building */
/// #   DrawParam::new();
/// let transform = param.to_matrix();
/// graphics::set_transform(ctx, transform);
/// # }
/// ```
pub fn set_transform<M>(context: &mut Context, transform: M)
where
    M: Into<mint::ColumnMatrix4<f32>>,
{
    let transform = transform.into();
    let gfx = &mut context.gfx_context;
    gfx.set_transform(Matrix4::from(transform));
}

/// Gets a copy of the context's current transform matrix
pub fn transform(context: &Context) -> mint::ColumnMatrix4<f32> {
    let gfx = &context.gfx_context;
    gfx.transform().into()
}

/// Premultiplies the given transform with the current model transform.
///
/// You must call [`apply_transformations(ctx)`](fn.apply_transformations.html)
/// after calling this to apply these changes and recalculate the
/// underlying MVP matrix.
///
/// A [`DrawParam`](struct.DrawParam.html) can be converted into an appropriate
/// transform matrix by turning it into a [`DrawTransform`](struct.DrawTransform.html):
///
/// ```rust,no_run
/// # use ggez::nalgebra as na;
/// # use ggez::*;
/// # use ggez::graphics::*;
/// # fn main() {
/// # let ctx = &mut ContextBuilder::new("foo", "bar").build().unwrap().0;
/// let param = /* DrawParam building */
/// #   DrawParam::new();
/// let transform = param.to_matrix();
/// graphics::mul_transform(ctx, transform);
/// # }
/// ```
pub fn mul_transform<M>(context: &mut Context, transform: M)
where
    M: Into<mint::ColumnMatrix4<f32>>,
{
    let transform = Matrix4::from(transform.into());
    let gfx = &mut context.gfx_context;
    let curr = gfx.transform();
    gfx.set_transform(curr * transform);
}

/// Sets the current model transform to the origin transform (no transformation)
///
/// You must call [`apply_transformations(ctx)`](fn.apply_transformations.html)
/// after calling this to apply these changes and recalculate the
/// underlying MVP matrix.
pub fn origin(context: &mut Context) {
    let gfx = &mut context.gfx_context;
    gfx.set_transform(Matrix4::identity());
}

/// Calculates the new total transformation (Model-View-Projection) matrix
/// based on the matrices at the top of the transform and view matrix stacks
/// and sends it to the graphics card.
pub fn apply_transformations(context: &mut Context) -> GameResult {
    let gfx = &mut context.gfx_context;
    gfx.calculate_transform_matrix();
    gfx.update_globals()
}

/// Sets the blend mode of the currently active shader program
pub fn set_blend_mode(ctx: &mut Context, mode: BlendMode) -> GameResult {
    ctx.gfx_context.set_blend_mode(mode)
}

/// Sets the window mode, such as the size and other properties.
///
/// Setting the window mode may have side effects, such as clearing
/// the screen or setting the screen coordinates viewport to some
/// undefined value (for example, the window was resized).  It is
/// recommended to call
/// [`set_screen_coordinates()`](fn.set_screen_coordinates.html) after
/// changing the window size to make sure everything is what you want
/// it to be.
pub fn set_mode(context: &mut Context, mode: WindowMode) -> GameResult {
    let gfx = &mut context.gfx_context;
    gfx.set_window_mode(mode)?;
    // Save updated mode.
    context.conf.window_mode = mode;
    Ok(())
}

/// Sets the window to fullscreen or back.
pub fn set_fullscreen(context: &mut Context, fullscreen: conf::FullscreenType) -> GameResult {
    let mut window_mode = context.conf.window_mode;
    window_mode.fullscreen_type = fullscreen;
    set_mode(context, window_mode)
}

/// Sets the window size/resolution to the specified width and height.
pub fn set_drawable_size(context: &mut Context, width: f32, height: f32) -> GameResult {
    let mut window_mode = context.conf.window_mode;
    window_mode.width = width;
    window_mode.height = height;
    set_mode(context, window_mode)
}

/// Sets whether or not the window is resizable.
pub fn set_resizable(context: &mut Context, resizable: bool) -> GameResult {
    let mut window_mode = context.conf.window_mode;
    window_mode.resizable = resizable;
    set_mode(context, window_mode)
}

/// Sets the window icon.
pub fn set_window_icon<P: AsRef<Path>>(context: &mut Context, path: Option<P>) -> GameResult<()> {
    let icon = match path {
        Some(p) => {
            let p: &Path = p.as_ref();
            Some(context::load_icon(p, &mut context.filesystem)?)
        }
        None => None,
    };
    context.gfx_context.window.set_window_icon(icon);
    Ok(())
}

/// Sets the window title.
pub fn set_window_title(context: &Context, title: &str) {
    context.gfx_context.window.set_title(title);
}

/// Returns a reference to the Glutin window.
/// Ideally you should not need to use this because ggez
/// would provide all the functions you need without having
/// to dip into Glutin itself.  But life isn't always ideal.
pub fn window(context: &Context) -> &glutin::WindowedContext {
    let gfx = &context.gfx_context;
    &gfx.window
}

/// Returns the size of the window in pixels as (width, height),
/// including borders, titlebar, etc.
/// Returns zeros if the window doesn't exist.
pub fn size(context: &Context) -> (f32, f32) {
    let gfx = &context.gfx_context;
    gfx.window
        .get_outer_size()
        .map(|logical_size| (logical_size.width as f32, logical_size.height as f32))
        .unwrap_or((0.0, 0.0))
}

/// Returns the size of the window's underlying drawable in pixels as (width, height).
/// Returns zeros if window doesn't exist.
pub fn drawable_size(context: &Context) -> (f32, f32) {
    let gfx = &context.gfx_context;
    gfx.window
        .get_inner_size()
        .map(|logical_size| (logical_size.width as f32, logical_size.height as f32))
        .unwrap_or((0.0, 0.0))
}

/// Returns raw `gfx-rs` state objects, if you want to use `gfx-rs` to write
/// your own graphics pipeline then this gets you the interfaces you need
/// to do so.
///
/// Returns all the relevant objects at once;
/// getting them one by one is awkward 'cause it tends to create double-borrows
/// on the Context object.
pub fn gfx_objects(
    context: &mut Context,
) -> (
    &mut <GlBackendSpec as BackendSpec>::Factory,
    &mut <GlBackendSpec as BackendSpec>::Device,
    &mut gfx::Encoder<
        <GlBackendSpec as BackendSpec>::Resources,
        <GlBackendSpec as BackendSpec>::CommandBuffer,
    >,
    gfx::handle::RawDepthStencilView<<GlBackendSpec as BackendSpec>::Resources>,
    gfx::handle::RawRenderTargetView<<GlBackendSpec as BackendSpec>::Resources>,
) {
    let gfx = &mut context.gfx_context;
    let f = &mut gfx.factory;
    let d = gfx.device.as_mut();
    let e = &mut gfx.encoder;
    let dv = gfx.depth_view.clone();
    let cv = gfx.data.out.clone();
    (f, d, e, dv, cv)
}

/// All types that can be drawn on the screen implement the `Drawable` trait.
pub trait Drawable {
    /// Draws the drawable onto the rendering target.
    fn draw(&self, ctx: &mut Context, param: DrawParam) -> GameResult;

    /// Returns a bounding box in the form of a `Rect`.
    ///
    /// It returns `Option` because some `Drawable`s may have no bounding box
    /// (an empty `SpriteBatch` for example).
    fn dimensions(&self, ctx: &mut Context) -> Option<Rect>;

    /// Sets the blend mode to be used when drawing this drawable.
    /// This overrides the general [`graphics::set_blend_mode()`](fn.set_blend_mode.html).
    /// If `None` is set, defers to the blend mode set by
    /// `graphics::set_blend_mode()`.
    fn set_blend_mode(&mut self, mode: Option<BlendMode>);

    /// Gets the blend mode to be used when drawing this drawable.
    fn blend_mode(&self) -> Option<BlendMode>;
}

/// Applies `DrawParam` to `Rect`.
pub fn transform_rect(rect: Rect, param: DrawParam) -> Rect {
    let w = param.src.w * param.scale.x * rect.w;
    let h = param.src.h * param.scale.y * rect.h;
    let offset_x = w * param.offset.x;
    let offset_y = h * param.offset.y;
    let dest_x = param.dest.x - offset_x;
    let dest_y = param.dest.y - offset_y;
    let mut r = Rect {
        w,
        h,
        x: dest_x + rect.x * param.scale.x,
        y: dest_y + rect.y * param.scale.y,
    };
    r.rotate(param.rotation);
    r
}

#[cfg(test)]
mod tests {
    use crate::graphics::{transform_rect, DrawParam, Rect};
    use approx::assert_relative_eq;
    use std::f32::consts::PI;

    #[test]
    fn headless_test_transform_rect() {
        {
            let r = Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            };
            let param = DrawParam::new();
            let real = transform_rect(r, param);
            let expected = r;
            assert_relative_eq!(real, expected);
        }
        {
            let r = Rect {
                x: -1.0,
                y: -1.0,
                w: 2.0,
                h: 1.0,
            };
            let param = DrawParam::new().scale([0.5, 0.5]);
            let real = transform_rect(r, param);
            let expected = Rect {
                x: -0.5,
                y: -0.5,
                w: 1.0,
                h: 0.5,
            };
            assert_relative_eq!(real, expected);
        }
        {
            let r = Rect {
                x: -1.0,
                y: -1.0,
                w: 1.0,
                h: 1.0,
            };
            let param = DrawParam::new().offset([0.5, 0.5]);
            let real = transform_rect(r, param);
            let expected = Rect {
                x: -1.5,
                y: -1.5,
                w: 1.0,
                h: 1.0,
            };
            assert_relative_eq!(real, expected);
        }
        {
            let r = Rect {
                x: 1.0,
                y: 0.0,
                w: 2.0,
                h: 1.0,
            };
            let param = DrawParam::new().rotation(PI * 0.5);
            let real = transform_rect(r, param);
            let expected = Rect {
                x: -1.0,
                y: 1.0,
                w: 1.0,
                h: 2.0,
            };
            assert_relative_eq!(real, expected);
        }
        {
            let r = Rect {
                x: -1.0,
                y: -1.0,
                w: 2.0,
                h: 1.0,
            };
            let param = DrawParam::new()
                .scale([0.5, 0.5])
                .offset([0.0, 1.0])
                .rotation(PI * 0.5);
            let real = transform_rect(r, param);
            let expected = Rect {
                x: 0.5,
                y: -0.5,
                w: 0.5,
                h: 1.0,
            };
            assert_relative_eq!(real, expected);
        }
    }
}
