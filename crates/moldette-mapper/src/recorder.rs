use std::borrow::Cow;

use moldette_core::Value;

use crate::op::CompiledOp;

// One execution path for map and explain; see #39.
pub(crate) trait Recorder<'a> {
    fn field(&mut self, name: &'a str);

    fn read(&mut self, field: &'a str, text: Option<&Cow<'a, str>>);

    fn constant(&mut self, value: &'a str);

    fn step(&mut self, op: &'a CompiledOp, output: Option<&Cow<'a, str>>);

    fn coerced(&mut self, value: Option<&Value<'a>>);
}

pub(crate) struct Discard;

impl<'a> Recorder<'a> for Discard {
    fn field(&mut self, _name: &'a str) {}

    fn read(&mut self, _field: &'a str, _text: Option<&Cow<'a, str>>) {}

    fn constant(&mut self, _value: &'a str) {}

    fn step(&mut self, _op: &'a CompiledOp, _output: Option<&Cow<'a, str>>) {}

    fn coerced(&mut self, _value: Option<&Value<'a>>) {}
}
