//! Gamepad-specific definitions

use gilrs::ev::filter::FilterFn;
use thiserror::Error;

use crate::core::{input::gamepad::GamepadAxis::LeftTrigger, math::Vec2};

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Element on a gamepad with a value either in `0.0..1.0` or `-1.0..1.0`
pub enum GamepadAxis {
    /// Left stick, horizontal
    LeftX = 0x00,
    /// Left stick, vertical
    LeftY = 0x01,
    /// Left trigger or Z
    LeftTrigger = 0x0a,
    /// Right stick, horizontal
    RightX = 0x10,
    /// Right stick, vertical
    RightY = 0x11,
    /// Right trigger or Z
    RightTrigger = 0x1a,
    /// D-Pad, horizontal
    DPadX = 0x20,
    /// D-Pad, vertical
    DPadY = 0x21,
}

impl TryFrom<gilrs::Axis> for GamepadAxis {
    type Error = UnknownGamepadAxis;
    fn try_from(value: gilrs::Axis) -> Result<Self, Self::Error> {
        match value {
            gilrs::Axis::LeftStickX => Ok(GamepadAxis::LeftX),
            gilrs::Axis::LeftStickY => Ok(GamepadAxis::LeftY),
            gilrs::Axis::LeftZ => Ok(GamepadAxis::LeftTrigger),
            gilrs::Axis::RightStickX => Ok(GamepadAxis::RightX),
            gilrs::Axis::RightStickY => Ok(GamepadAxis::RightY),
            gilrs::Axis::RightZ => Ok(GamepadAxis::RightTrigger),
            gilrs::Axis::DPadX => Ok(GamepadAxis::DPadX),
            gilrs::Axis::DPadY => Ok(GamepadAxis::DPadY),
            _ => Err(UnknownGamepadAxis),
        }
    }
}

#[derive(Debug, Error)]
#[error("unknown gamepad axis")]
#[allow(missing_docs)]
pub struct UnknownGamepadAxis;

#[repr(u16)]
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Element on a gamepad with a strictly binary state.
pub enum GamepadBinary {
    /// The button at the top.
    ///
    /// Xbox / Steam: Y
    ///
    /// PlayStation: Triangle
    ///
    /// Nintendo: X
    North,
    /// The button at the bottom.
    ///
    /// Xbox / Steam: X
    ///
    /// PlayStation: Cross (X)
    ///
    /// Nintendo: B
    South,
    /// The button on the right.
    ///
    /// Xbox / Steam: B
    ///
    /// PlayStation: Circle
    ///
    /// Nintendo: A
    East,
    /// The button on the left.
    ///
    /// Xbox / Steam: X
    ///
    /// PlayStation: Square
    ///
    /// Nintendo: Y
    West,

    LeftBumper,
    RightBumper,

    /// Triggered by pressing down on the left joystick
    LeftPress,
    /// Triggered by pressing down on the right joystick
    RightPress,

    Select,
    Start,
    Mode,
}

impl GamepadBinary {
    /// Is one of the north, east, south, or west buttons.
    pub fn is_action(&self) -> bool {
        matches!(self, Self::North | Self::South | Self::East | Self::West)
    }

    /// Is one of the select, start, or mode buttons
    pub fn is_menu(&self) -> bool {
        matches!(self, Self::Select | Self::Start | Self::Mode)
    }
}

impl TryFrom<gilrs::Button> for GamepadBinary {
    type Error = UnknownGamepadButton;
    fn try_from(value: gilrs::Button) -> Result<Self, Self::Error> {
        match value {
            gilrs::Button::North => Ok(Self::North),
            gilrs::Button::South => Ok(Self::South),
            gilrs::Button::East => Ok(Self::East),
            gilrs::Button::West => Ok(Self::West),
            gilrs::Button::LeftTrigger2 => Ok(Self::LeftBumper),
            gilrs::Button::RightTrigger2 => Ok(Self::RightBumper),
            gilrs::Button::LeftThumb => Ok(Self::LeftPress),
            gilrs::Button::RightThumb => Ok(Self::RightPress),
            gilrs::Button::Select => Ok(Self::Select),
            gilrs::Button::Start => Ok(Self::Start),
            gilrs::Button::Mode => Ok(Self::Mode),
            _ => Err(UnknownGamepadButton),
        }
    }
}

#[derive(Debug, Error)]
#[error("unknown gamepad button")]
#[allow(missing_docs)]
pub struct UnknownGamepadButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
/// 2D element on a gamepad, e.g. a [`GamepadAxis`] with both X and Y
pub enum Gamepad2d {
    LeftStick = 0x0f,
    RightStick = 0x1f,
    DPad = 0x2f,
}

#[derive(Debug, Clone, Copy)]
pub struct GamepadState {
    left_stick: Vec2,
    right_stick: Vec2,
    dpad: Vec2,

    left_trigger: f32,
    right_trigger: f32,

    is_north_pressed: bool,
    is_south_pressed: bool,
    is_east_pressed: bool,
    is_west_pressed: bool,

    is_left_stick_pressed: bool,
    is_right_stick_pressed: bool,

    is_left_bumper_pressed: bool,
    is_right_bumper_pressed: bool,

    is_select_pressed: bool,
    is_start_pressed: bool,
    is_mode_pressed: bool,
}

impl GamepadState {
    fn delta(&self, previous: &GamepadState) -> GamepadState {
        GamepadState {
            left_stick: self.left_stick - previous.left_stick,
            right_stick: self.right_stick - previous.right_stick,
            dpad: self.dpad - previous.dpad,

            left_trigger: self.left_trigger - previous.left_trigger,
            right_trigger: self.right_trigger - previous.right_trigger,

            is_north_pressed: self.is_north_pressed != previous.is_north_pressed,
            is_south_pressed: self.is_south_pressed != previous.is_south_pressed,
            is_east_pressed: self.is_east_pressed != previous.is_east_pressed,
            is_west_pressed: self.is_west_pressed != previous.is_west_pressed,

            is_left_stick_pressed: self.is_left_stick_pressed != previous.is_left_stick_pressed,
            is_right_stick_pressed: self.is_right_stick_pressed != previous.is_right_stick_pressed,

            is_left_bumper_pressed: self.is_left_bumper_pressed != previous.is_left_bumper_pressed,
            is_right_bumper_pressed: self.is_right_bumper_pressed
                != previous.is_right_bumper_pressed,

            is_select_pressed: self.is_select_pressed != previous.is_select_pressed,
            is_start_pressed: self.is_start_pressed != previous.is_start_pressed,
            is_mode_pressed: self.is_mode_pressed != previous.is_mode_pressed,
        }
    }

    fn from_gamepad(gamepad: gilrs::Gamepad<'_>) -> Self {
        Self {
            left_stick: Vec2::new(
                gamepad.value(gilrs::Axis::LeftStickX),
                gamepad.value(gilrs::Axis::LeftStickY),
            ),
            right_stick: Vec2::new(
                gamepad.value(gilrs::Axis::RightStickX),
                gamepad.value(gilrs::Axis::RightStickY),
            ),
            dpad: Vec2::new(
                gamepad.value(gilrs::Axis::DPadX),
                gamepad.value(gilrs::Axis::DPadY),
            ),

            left_trigger: gamepad
                .value(gilrs::Axis::LeftZ)
                .max(gamepad.is_pressed(gilrs::Button::LeftTrigger) as u8 as f32),
            right_trigger: gamepad
                .value(gilrs::Axis::RightZ)
                .max(gamepad.is_pressed(gilrs::Button::RightTrigger) as u8 as f32),

            is_north_pressed: gamepad.is_pressed(gilrs::Button::North),
            is_south_pressed: gamepad.is_pressed(gilrs::Button::South),
            is_east_pressed: gamepad.is_pressed(gilrs::Button::East),
            is_west_pressed: gamepad.is_pressed(gilrs::Button::West),

            is_left_stick_pressed: gamepad.is_pressed(gilrs::Button::LeftThumb),
            is_right_stick_pressed: gamepad.is_pressed(gilrs::Button::RightThumb),

            is_left_bumper_pressed: gamepad.is_pressed(gilrs::Button::LeftTrigger2),
            is_right_bumper_pressed: gamepad.is_pressed(gilrs::Button::RightTrigger2),

            is_select_pressed: gamepad.is_pressed(gilrs::Button::Select),
            is_start_pressed: gamepad.is_pressed(gilrs::Button::Start),
            is_mode_pressed: gamepad.is_pressed(gilrs::Button::Mode),
        }
    }

    const fn empty_delta() -> Self {
        Self {
            left_stick: Vec2::ZERO,
            right_stick: Vec2::ZERO,
            dpad: Vec2::ZERO,
            left_trigger: 0.0,
            right_trigger: 0.0,
            is_north_pressed: false,
            is_south_pressed: false,
            is_east_pressed: false,
            is_west_pressed: false,
            is_left_stick_pressed: false,
            is_right_stick_pressed: false,
            is_left_bumper_pressed: false,
            is_right_bumper_pressed: false,
            is_select_pressed: false,
            is_start_pressed: false,
            is_mode_pressed: false,
        }
    }
}

pub(crate) enum SetButtonResult {
    BinaryDelta(bool, GamepadBinary),
    AxisDeltaVal(f32, f32, GamepadAxis),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GamepadId(pub(super) gilrs::GamepadId);

pub struct Gamepad {
    this_state: GamepadState,
    last_state: GamepadState,
    state_delta: GamepadState,
}

impl Gamepad {
    pub(crate) fn from_gamepad(gamepad: gilrs::Gamepad<'_>) -> Self {
        let state = GamepadState::from_gamepad(gamepad);
        Self {
            this_state: state,
            last_state: state,
            state_delta: GamepadState::empty_delta(),
        }
    }

    fn update_delta(&mut self) {
        self.state_delta = self.this_state.delta(&self.last_state);
    }

    pub(crate) fn advance_state(&mut self) {
        self.last_state = self.this_state;
    }

    pub(crate) fn set_axis(
        &mut self,
        axis: GamepadAxis,
        value: f32,
    ) -> Result<f32, UnknownGamepadAxis> {
        match axis {
            GamepadAxis::DPadX => {
                self.this_state.dpad.x = value;
                self.state_delta.dpad.x = value - self.last_state.dpad.x;
                Ok(self.state_delta.dpad.x)
            }
            GamepadAxis::DPadY => {
                self.this_state.dpad.y = value;
                self.state_delta.dpad.y = value - self.last_state.dpad.y;
                Ok(self.state_delta.dpad.y)
            }
            GamepadAxis::LeftX => {
                self.this_state.left_stick.x = value;
                self.state_delta.left_stick.x = value - self.last_state.left_stick.x;
                Ok(self.state_delta.left_stick.x)
            }
            GamepadAxis::LeftY => {
                self.this_state.left_stick.y = value;
                self.state_delta.left_stick.y = value - self.last_state.left_stick.y;
                Ok(self.state_delta.left_stick.y)
            }
            GamepadAxis::RightX => {
                self.this_state.right_stick.x = value;
                self.state_delta.right_stick.x = value - self.last_state.right_stick.x;
                Ok(self.state_delta.right_stick.x)
            }
            GamepadAxis::RightY => {
                self.this_state.right_stick.y = value;
                self.state_delta.right_stick.y = value - self.last_state.right_stick.y;
                Ok(self.state_delta.right_stick.y)
            }
            GamepadAxis::LeftTrigger => {
                self.this_state.left_trigger = value;
                self.state_delta.left_trigger = value - self.last_state.left_trigger;
                Ok(self.state_delta.left_trigger)
            }
            GamepadAxis::RightTrigger => {
                self.this_state.right_trigger = value;
                self.state_delta.right_trigger = value - self.last_state.right_trigger;
                Ok(self.state_delta.right_trigger)
            }
        }
    }

    pub(crate) fn set_button(&mut self, button: gilrs::Button, pressed: bool) -> SetButtonResult {
        match button {
            gilrs::Button::North => {
                self.this_state.is_north_pressed = pressed;
                self.state_delta.is_north_pressed = pressed != self.last_state.is_north_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_north_pressed,
                    GamepadBinary::North,
                )
            }
            gilrs::Button::South => {
                self.this_state.is_south_pressed = pressed;
                self.state_delta.is_south_pressed = pressed != self.last_state.is_south_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_south_pressed,
                    GamepadBinary::South,
                )
            }
            gilrs::Button::East => {
                self.this_state.is_east_pressed = pressed;
                self.state_delta.is_east_pressed = pressed != self.last_state.is_east_pressed;
                SetButtonResult::BinaryDelta(self.state_delta.is_east_pressed, GamepadBinary::East)
            }
            gilrs::Button::West => {
                self.this_state.is_west_pressed = pressed;
                self.state_delta.is_west_pressed = pressed != self.last_state.is_west_pressed;
                SetButtonResult::BinaryDelta(self.state_delta.is_west_pressed, GamepadBinary::West)
            }
            gilrs::Button::LeftThumb => {
                self.this_state.is_left_stick_pressed = pressed;
                self.state_delta.is_left_stick_pressed =
                    pressed != self.last_state.is_left_stick_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_left_stick_pressed,
                    GamepadBinary::LeftPress,
                )
            }
            gilrs::Button::LeftTrigger2 => {
                self.this_state.is_left_bumper_pressed = pressed;
                self.state_delta.is_left_bumper_pressed =
                    pressed != self.last_state.is_left_bumper_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_left_bumper_pressed,
                    GamepadBinary::LeftBumper,
                )
            }
            gilrs::Button::RightThumb => {
                self.this_state.is_right_stick_pressed = pressed;
                self.state_delta.is_right_stick_pressed =
                    pressed != self.last_state.is_right_stick_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_right_stick_pressed,
                    GamepadBinary::RightPress,
                )
            }
            gilrs::Button::RightTrigger2 => {
                self.this_state.is_right_bumper_pressed = pressed;
                self.state_delta.is_right_bumper_pressed =
                    pressed != self.last_state.is_right_bumper_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_right_bumper_pressed,
                    GamepadBinary::RightBumper,
                )
            }
            gilrs::Button::Mode => {
                self.this_state.is_north_pressed = pressed;
                self.state_delta.is_mode_pressed = pressed != self.last_state.is_mode_pressed;
                SetButtonResult::BinaryDelta(self.state_delta.is_mode_pressed, GamepadBinary::Mode)
            }
            gilrs::Button::Start => {
                self.this_state.is_start_pressed = pressed;
                self.state_delta.is_start_pressed = pressed != self.last_state.is_start_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_start_pressed,
                    GamepadBinary::Start,
                )
            }
            gilrs::Button::Select => {
                self.this_state.is_select_pressed = pressed;
                self.state_delta.is_select_pressed = pressed != self.last_state.is_select_pressed;
                SetButtonResult::BinaryDelta(
                    self.state_delta.is_select_pressed,
                    GamepadBinary::Select,
                )
            }
            // subtract/add, that way if you press left (-1) + right (+1=0),
            // then let go of left (+1=1)
            gilrs::Button::DPadDown => {
                if pressed {
                    self.this_state.dpad.y = (self.this_state.dpad.y - 1.0).max(-1.0);
                } else {
                    self.this_state.dpad.y = (self.this_state.dpad.y + 1.0).max(1.0);
                }
                self.state_delta.dpad.y = self.this_state.dpad.y - self.last_state.dpad.y;
                SetButtonResult::AxisDeltaVal(self.state_delta.dpad.y, self.this_state.dpad.y, GamepadAxis::DPadY)
            }
            gilrs::Button::DPadUp => {
                if pressed {
                    self.this_state.dpad.y = (self.this_state.dpad.y + 1.0).max(1.0);
                } else {
                    self.this_state.dpad.y = (self.this_state.dpad.y - 1.0).max(-1.0);
                }
                self.state_delta.dpad.y = self.this_state.dpad.y - self.last_state.dpad.y;
                SetButtonResult::AxisDeltaVal(self.state_delta.dpad.y, self.this_state.dpad.y, GamepadAxis::DPadY)
            }
            gilrs::Button::DPadLeft => {
                if pressed {
                    self.this_state.dpad.x = (self.this_state.dpad.x - 1.0).max(-1.0);
                } else {
                    self.this_state.dpad.x = (self.this_state.dpad.x + 1.0).max(1.0);
                }
                self.state_delta.dpad.x = self.this_state.dpad.x - self.last_state.dpad.x;
                SetButtonResult::AxisDeltaVal(self.state_delta.dpad.x, self.this_state.dpad.x, GamepadAxis::DPadX)
            }
            gilrs::Button::DPadRight => {
                if pressed {
                    self.this_state.dpad.x = (self.this_state.dpad.x + 1.0).max(1.0);
                } else {
                    self.this_state.dpad.x = (self.this_state.dpad.x - 1.0).max(-1.0);
                }
                self.state_delta.dpad.x = self.this_state.dpad.x - self.last_state.dpad.x;
                SetButtonResult::AxisDeltaVal(self.state_delta.dpad.x, self.this_state.dpad.x, GamepadAxis::DPadX)
            }
            _ => SetButtonResult::Unknown,
        }
    }

    pub fn delta(&self) -> &GamepadState {
        &self.state_delta
    }
}

pub(crate) struct TriggerAsAnalog;

impl FilterFn for TriggerAsAnalog {
    fn filter(&self, ev: Option<gilrs::Event>, gilrs: &mut gilrs::Gilrs) -> Option<gilrs::Event> {
        let mut ev = ev?;

        match ev.event {
            gilrs::EventType::ButtonPressed(gilrs::Button::LeftTrigger, code) => {
                if gilrs.gamepad(ev.id).axis_code(gilrs::Axis::LeftZ).is_some() {
                    return Some(gilrs::Event::new(ev.id, gilrs::EventType::Dropped));
                } else {
                    ev.event = gilrs::EventType::AxisChanged(gilrs::Axis::LeftZ, 1.0, code)
                }
            }
            gilrs::EventType::ButtonPressed(gilrs::Button::RightTrigger, code) => {
                if gilrs
                    .gamepad(ev.id)
                    .axis_code(gilrs::Axis::RightZ)
                    .is_some()
                {
                    return Some(gilrs::Event::new(ev.id, gilrs::EventType::Dropped));
                } else {
                    ev.event = gilrs::EventType::AxisChanged(gilrs::Axis::RightZ, 1.0, code)
                }
            }

            gilrs::EventType::ButtonReleased(gilrs::Button::LeftTrigger, code) => {
                if gilrs.gamepad(ev.id).axis_code(gilrs::Axis::LeftZ).is_some() {
                    return Some(gilrs::Event::new(ev.id, gilrs::EventType::Dropped));
                } else {
                    ev.event = gilrs::EventType::AxisChanged(gilrs::Axis::LeftZ, 0.0, code)
                }
            }
            gilrs::EventType::ButtonReleased(gilrs::Button::RightTrigger, code) => {
                if gilrs
                    .gamepad(ev.id)
                    .axis_code(gilrs::Axis::RightZ)
                    .is_some()
                {
                    return Some(gilrs::Event::new(ev.id, gilrs::EventType::Dropped));
                } else {
                    ev.event = gilrs::EventType::AxisChanged(gilrs::Axis::RightZ, 0.0, code)
                }
            }
            _ => {}
        }

        Some(ev)
    }
}
