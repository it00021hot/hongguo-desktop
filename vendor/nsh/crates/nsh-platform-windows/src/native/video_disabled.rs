use std::{cell::RefCell, rc::Rc, sync::Arc};

use nsh_core::{EventKind, NodeId, VideoProps};

use super::{Emitter, error};
use crate::{image::AssetBase, render::DrawCx, surface::Surface, view::Hosted};

pub(crate) struct VideoItem {
    emitter: Emitter,
    source: RefCell<Option<Arc<str>>>,
}

impl VideoItem {
    pub(crate) fn new(surface: &Rc<Surface>, id: NodeId) -> Rc<Self> {
        Rc::new(Self {
            emitter: Emitter::new(surface, id),
            source: RefCell::new(None),
        })
    }

    pub(crate) fn set(
        self: &Rc<Self>,
        _surface: &Rc<Surface>,
        props: &VideoProps,
        _base: Option<&AssetBase>,
    ) {
        if *self.source.borrow() == props.source {
            return;
        }
        *self.source.borrow_mut() = props.source.clone();
        if props.source.is_some() {
            self.emitter.emit(EventKind::Error, || {
                error("NSH system media support is not enabled in this build".into())
            });
        }
    }

    pub(crate) fn close(&self) {
        self.source.borrow_mut().take();
    }

    pub(crate) fn seek(self: &Rc<Self>, _seconds: f64) {}
}

impl Hosted for VideoItem {
    fn draw(&self, _cx: &mut DrawCx<'_>, _size: (f32, f32)) {}

    fn hit(&self, _x: f32, _y: f32, _size: (f32, f32)) -> bool {
        false
    }
}
