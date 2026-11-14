use crate::PlayerController;
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
    sel,
};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes};
use std::cell::Cell;

pub struct DisplayClock {
    link: Retained<AnyObject>,
    maximum_fps: Cell<u32>,
    applied_fps: Cell<u32>,
}

impl DisplayClock {
    /// # Safety
    /// Call on the main thread with a display-link factory and a target implementing hongguoFrameTick:.
    pub unsafe fn new(owner: &AnyObject, target: &AnyObject) -> Self {
        let link: Retained<AnyObject> = unsafe {
            msg_send![owner, displayLinkWithTarget: target, selector: sel!(hongguoFrameTick:)]
        };
        unsafe {
            let _: () = msg_send![&link, setPaused: Bool::YES];
            let _: () = msg_send![&link, addToRunLoop: &*NSRunLoop::mainRunLoop(), forMode: NSRunLoopCommonModes];
        }
        Self {
            link,
            maximum_fps: Cell::new(60),
            applied_fps: Cell::new(0),
        }
    }

    pub fn set_active(&self, active: bool, controller: &PlayerController) {
        unsafe {
            let _: () = msg_send![&self.link, setPaused: Bool::new(!active)];
        }
        controller.set_display_clock_active(active);
    }

    pub fn set_maximum_fps(&self, fps: u32) {
        if fps > 0 {
            self.maximum_fps.set(fps);
        }
    }

    pub fn tick(&self, controller: &PlayerController) {
        let required = controller.preferred_frame_rate();
        let maximum = self.maximum_fps.get();
        let preferred = if required == 0 {
            0
        } else {
            maximum / (maximum / required.min(maximum)).max(1)
        };
        if self.applied_fps.replace(preferred) != preferred {
            unsafe {
                let _: () = msg_send![&self.link, setPreferredFramesPerSecond: preferred as isize];
            }
        }
        let deadline: f64 = unsafe { msg_send![&self.link, targetTimestamp] };
        if controller.display_tick(deadline).is_err() {
            self.invalidate();
        }
    }

    pub fn invalidate(&self) {
        unsafe {
            let _: () = msg_send![&self.link, invalidate];
        }
    }
}

impl Drop for DisplayClock {
    fn drop(&mut self) {
        self.invalidate();
    }
}

unsafe extern "C" {
    fn CACurrentMediaTime() -> f64;
}

pub(crate) fn presentation_lead(deadline: f64) -> f64 {
    if !deadline.is_finite() {
        return 0.0;
    }
    (deadline - unsafe { CACurrentMediaTime() }).max(0.0)
}
