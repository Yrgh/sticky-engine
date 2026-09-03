//! Input events such as mouse movement, keys being pressed, and focus changes.

#![allow(missing_docs, unused)]

use std::collections::HashMap;

use gilrs::Filter;
use sticky_engine_macros::slot_def;
use winit::{
    event::{DeviceEvent, DeviceId, MouseScrollDelta, WindowEvent},
    keyboard::PhysicalKey,
    window::Window,
};

use crate::core::{
    input::gamepad::{Gamepad, GamepadAxis, GamepadBinary, GamepadId, SetButtonResult}, math::Vec2, window::WindowId, world::World,
};

pub mod gamepad;

#[allow(missing_docs)]
/// The mouse button that was pressed.
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Forward,
    Backward,
    Other(u16),
}

impl From<winit::event::MouseButton> for MouseButton {
    fn from(value: winit::event::MouseButton) -> Self {
        match value {
            winit::event::MouseButton::Left => Self::Left,
            winit::event::MouseButton::Right => Self::Right,
            winit::event::MouseButton::Middle => Self::Middle,
            winit::event::MouseButton::Forward => Self::Forward,
            winit::event::MouseButton::Back => Self::Backward,
            winit::event::MouseButton::Other(b) => Self::Other(b),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// Filters [`MouseScrollDelta`] into a single [`Vec2`].
pub struct ScrollFilter {
    threshold: f32,
}

impl ScrollFilter {
    /// Create a new filter.
    ///
    /// `threshold` is the number of pixels per unit.
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    /// Counts the number of actions to perform, using [`f32::round()`].
    ///
    /// For line deltas, each line is counted as one action.
    ///
    /// For pixel deltas, every `threshold` pixels is counted as one action.
    pub fn count_round(&self, delta: MouseScrollDelta) -> Vec2 {
        match delta {
            MouseScrollDelta::LineDelta(lx, ly) => Vec2::new(lx.round(), ly.round()),
            MouseScrollDelta::PixelDelta(dp) => Vec2::new(
                (dp.x as f32 / self.threshold).round(),
                (dp.y as f32 / self.threshold).round(),
            ),
        }
    }
}

#[allow(missing_docs)]
#[non_exhaustive]
/// Input-related event received, either global or for the current window.
pub enum InputEvent {
    MouseMotion {
        /// The number of pixels travelled
        delta: Vec2,
    },
    MouseButton {
        button: MouseButton,
        state: bool,
    },
    MouseWheel(MouseScrollDelta),
    GamepadAxis {
        gamepad_id: GamepadId,
        axis: GamepadAxis,
        delta: f32,
        new_value: f32,
    },
    GamepadButton {
        gamepad_id: GamepadId,
        button: GamepadBinary,
        new_state: bool,
    },
    /// Emitted when a key is pressed or released. The engine filters out repetitions.
    ///
    /// This input may or may not correspond with a `WinitKeyEvent`, which may
    /// come any time before or after.
    KeyPhysical {
        key: PhysicalKey,
        state: bool,
    },
    /// Only emitted on the current window.
    WinitKeyEvent(winit::event::KeyEvent),
    /// Only emitted on the current window.
    FocusChanged {
        state: bool,
    },
    /// Only emitted on the current window.
    CloseRequested,
}

impl InputEvent {
    /// Returns `true` if the event should be emitted on all `Level`s.
    pub fn is_global(&self) -> bool {
        match self {
            InputEvent::MouseMotion { .. } |
            InputEvent::MouseButton { .. } |
            InputEvent::MouseWheel { .. } |
            InputEvent::GamepadAxis { .. } |
            InputEvent::GamepadButton { .. } |
            InputEvent::KeyPhysical { .. } => true,
            InputEvent::WinitKeyEvent(_) |
            InputEvent::FocusChanged { .. } |
            InputEvent::CloseRequested => false
        }
    }
}

pub(crate) struct InputHandler {
    known_key_states: HashMap<PhysicalKey, bool>,
    controllers: HashMap<GamepadId, Gamepad>,
    gilrs: gilrs::Gilrs,
}

impl InputHandler {
    pub(crate) fn new() -> Result<Self, Box<gilrs::Error>> {
        Ok(Self {
            known_key_states: HashMap::new(),
            controllers: HashMap::new(),
            gilrs: gilrs::GilrsBuilder::new().with_default_filters(false).build()?
        })
    }

    pub(crate) fn window_input(
        &mut self,
        input: WindowEvent,
        cursor: &mut Option<Vec2>,
    ) -> Option<InputEvent> {
        match input {
            WindowEvent::CursorMoved { position, .. } => {
                cursor.replace(Vec2::new(position.x as f32, position.y as f32));
                None
            }
            WindowEvent::CursorLeft { .. } => {
                cursor.take();
                None
            }
            WindowEvent::KeyboardInput { event, .. } => Some(InputEvent::WinitKeyEvent(event)),
            _ => None,
        }
    }

    pub(crate) fn device_input(&mut self, input: DeviceEvent) -> Option<InputEvent> {
        match input {
            DeviceEvent::Key(key) => Some(InputEvent::KeyPhysical {
                key: key.physical_key,
                state: key.state.is_pressed(),
            }),
            DeviceEvent::MouseMotion { delta } => Some(InputEvent::MouseMotion {
                delta: Vec2::new(delta.0 as f32, delta.1 as f32),
            }),
            _ => None,
        }
    }

    pub(crate) fn poll_controller_input(&mut self) -> Option<InputEvent> {
        let input = self.gilrs.next_event().filter_ev(&gamepad::TriggerAsAnalog, &mut self.gilrs)?;
        
        match input.event {
            gilrs::ev::EventType::AxisChanged(axis, value, _) => {
                let axis: GamepadAxis = axis.try_into().ok()?;

                let gamepad = self
                    .controllers
                    .entry(GamepadId(input.id))
                    .or_insert_with(|| Gamepad::from_gamepad(self.gilrs.gamepad(input.id)));

                if let Ok(delta) = gamepad.set_axis(axis, value) {
                    Some(InputEvent::GamepadAxis {
                        gamepad_id: GamepadId(input.id),
                        axis,
                        delta,
                        new_value: value,
                    })
                } else {
                    None
                }
            }
            gilrs::ev::EventType::ButtonPressed(button, _) => {
                let gamepad = self
                    .controllers
                    .entry(GamepadId(input.id))
                    .or_insert_with(|| Gamepad::from_gamepad(self.gilrs.gamepad(input.id)));

                match gamepad.set_button(button, true) {
                    SetButtonResult::BinaryDelta(true, button) => Some(InputEvent::GamepadButton {
                        gamepad_id: GamepadId(input.id),
                        button,
                        new_state: true,
                    }),
                    SetButtonResult::AxisDeltaVal(delta, new_value, axis) => {
                        Some(InputEvent::GamepadAxis {
                            gamepad_id: GamepadId(input.id),
                            axis,
                            delta,
                            new_value,
                        })
                    }
                    _ => None,
                }
            }
            gilrs::ev::EventType::ButtonReleased(button, _) => {
                let gamepad = self
                    .controllers
                    .entry(GamepadId(input.id))
                    .or_insert_with(|| Gamepad::from_gamepad(self.gilrs.gamepad(input.id)));

                match gamepad.set_button(button, false) {
                    SetButtonResult::BinaryDelta(true, button) => Some(InputEvent::GamepadButton {
                        gamepad_id: GamepadId(input.id),
                        button,
                        new_state: false,
                    }),
                    SetButtonResult::AxisDeltaVal(delta, new_value, axis) => {
                        Some(InputEvent::GamepadAxis {
                            gamepad_id: GamepadId(input.id),
                            axis,
                            delta,
                            new_value,
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn next_group(&mut self) {
        for controller in self.controllers.values_mut() {
            controller.advance_state();
        }
    }
}

#[slot_def]
/// Slot for Components that respond to raw input.
/// 
/// Unlike `idle` or `pre_phys`, most Components will never do anything with
/// input, and since it won't walk the tree, it will be even faster, which is
/// necessary due to the potential frequency of events.
pub trait SInputReceiver {
    fn raw_input(&mut self, world: &World, event: &InputEvent);
}