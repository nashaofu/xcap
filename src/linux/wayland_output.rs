use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
    },
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};

use crate::error::{XCapError, XCapResult};

#[derive(Debug)]
struct OutputInfo {
    output: WlOutput,
    name: Option<String>,
    logical_position: Option<(i32, i32)>,
    logical_size: Option<(i32, i32)>,
    physical_size: Option<(i32, i32)>,
    transform: wl_output::Transform,
}

impl OutputInfo {
    fn new(output: WlOutput) -> Self {
        Self {
            output,
            name: None,
            logical_position: None,
            logical_size: None,
            physical_size: None,
            transform: wl_output::Transform::Normal,
        }
    }

    fn scale_factor(&self) -> Option<f32> {
        let (physical_width, physical_height) = self.physical_size?;
        let (logical_width, logical_height) = self.logical_size?;
        if physical_width <= 0 || physical_height <= 0 || logical_width <= 0 || logical_height <= 0
        {
            return None;
        }

        let transformed_height = match self.transform {
            wl_output::Transform::_90
            | wl_output::Transform::_270
            | wl_output::Transform::Flipped90
            | wl_output::Transform::Flipped270 => physical_width,
            _ => physical_height,
        };

        Some(transformed_height as f32 / logical_height as f32)
    }
}

#[derive(Debug, Default)]
struct OutputState {
    output_manager: Option<ZxdgOutputManagerV1>,
    outputs: Vec<OutputInfo>,
}

impl Dispatch<WlRegistry, ()> for OutputState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue_handle: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };

        if interface == WlOutput::interface().name {
            let output_index = state.outputs.len();
            let output =
                registry.bind::<WlOutput, _, _>(name, version.min(4), queue_handle, output_index);
            state.outputs.push(OutputInfo::new(output));
        } else if interface == ZxdgOutputManagerV1::interface().name
            && state.output_manager.is_none()
        {
            state.output_manager = Some(registry.bind::<ZxdgOutputManagerV1, _, _>(
                name,
                version.min(3),
                queue_handle,
                (),
            ));
        }
    }
}

impl Dispatch<WlOutput, usize> for OutputState {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        output_index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*output_index) else {
            return;
        };

        match event {
            wl_output::Event::Name { name } => {
                output.name = Some(name);
            }
            wl_output::Event::Geometry { transform, .. } => {
                if let WEnum::Value(transform) = transform {
                    output.transform = transform;
                }
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } => {
                if matches!(flags, WEnum::Value(flags) if flags.contains(wl_output::Mode::Current))
                {
                    output.physical_size = Some((width, height));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for OutputState {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: <ZxdgOutputManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgOutputV1, usize> for OutputState {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        output_index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*output_index) else {
            return;
        };

        match event {
            zxdg_output_v1::Event::Name { name } => {
                output.name = Some(name);
            }
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.logical_position = Some((x, y));
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical_size = Some((width, height));
            }
            _ => {}
        }
    }
}

fn query_outputs() -> XCapResult<Vec<OutputInfo>> {
    let connection = Connection::connect_to_env().map_err(XCapError::new)?;
    let mut event_queue = connection.new_event_queue::<OutputState>();
    let queue_handle = event_queue.handle();
    let mut state = OutputState::default();

    connection.display().get_registry(&queue_handle, ());
    event_queue.roundtrip(&mut state).map_err(XCapError::new)?;

    let output_manager = state
        .output_manager
        .clone()
        .ok_or_else(|| XCapError::new("Wayland compositor does not support xdg-output"))?;
    let xdg_outputs = state
        .outputs
        .iter()
        .enumerate()
        .map(|(output_index, output)| {
            output_manager.get_xdg_output(&output.output, &queue_handle, output_index)
        })
        .collect::<Vec<_>>();

    event_queue.roundtrip(&mut state).map_err(XCapError::new)?;

    for xdg_output in xdg_outputs {
        xdg_output.destroy();
    }
    output_manager.destroy();

    Ok(state.outputs)
}

pub(crate) fn max_scale_factor() -> XCapResult<f32> {
    query_outputs()?
        .iter()
        .filter_map(OutputInfo::scale_factor)
        .reduce(f32::max)
        .ok_or_else(|| XCapError::new("No valid Wayland output scale"))
}

pub(crate) fn desktop_bounds() -> XCapResult<(i64, i64, i64, i64)> {
    let outputs = query_outputs()?;
    let mut bounds: Option<(i64, i64, i64, i64)> = None;
    for output in outputs {
        let (x, y) = output
            .logical_position
            .ok_or_else(|| XCapError::new("Missing Wayland output position"))?;
        let (width, height) = output
            .logical_size
            .filter(|&(width, height)| width > 0 && height > 0)
            .ok_or_else(|| XCapError::new("Missing Wayland output size"))?;
        let rect = (
            i64::from(x),
            i64::from(y),
            i64::from(x) + i64::from(width),
            i64::from(y) + i64::from(height),
        );
        bounds = Some(match bounds {
            Some((left, top, right, bottom)) => (
                left.min(rect.0),
                top.min(rect.1),
                right.max(rect.2),
                bottom.max(rect.3),
            ),
            None => rect,
        });
    }
    bounds.ok_or_else(|| XCapError::new("No Wayland outputs"))
}

pub(crate) fn monitor_geometry(name: &str) -> XCapResult<(i32, i32, u32, u32, f32)> {
    let outputs = query_outputs()?;
    let output = outputs
        .iter()
        .find(|output| output.name.as_deref() == Some(name))
        .or_else(|| (outputs.len() == 1).then(|| &outputs[0]))
        .ok_or_else(|| XCapError::new(format!("Cannot match XRandR output {name} to Wayland")))?;
    let (x, y) = output
        .logical_position
        .ok_or_else(|| XCapError::new("Missing Wayland output position"))?;
    let (width, height) = output
        .logical_size
        .filter(|&(width, height)| width > 0 && height > 0)
        .ok_or_else(|| XCapError::new("Missing Wayland output size"))?;
    let scale = output
        .scale_factor()
        .ok_or_else(|| XCapError::new("Missing Wayland output scale"))?;
    Ok((x, y, width as u32, height as u32, scale))
}
