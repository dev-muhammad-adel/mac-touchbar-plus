use input::{
    event::{
        Event,
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot}
    }
};
use input_linux::uinput::UInputHandle;
use std::collections::HashMap;
use crate::LayerKey;
use crate::layers::FunctionLayer;
use input_linux::Key;

// Static HashMap for flat touch slots
static mut FLAT_TOUCHES: Option<HashMap<u32, (LayerKey, &'static str, usize)>> = None;

pub struct FlatTouchHandler;

impl FlatTouchHandler {
    fn get_touches() -> &'static mut HashMap<u32, (LayerKey, &'static str, usize)> {
        unsafe {
            if FLAT_TOUCHES.is_none() {
                FLAT_TOUCHES = Some(HashMap::new());
            }
            FLAT_TOUCHES.as_mut().unwrap()
        }
    }

    /// Handles all touch events for flat buttons in non-Media layers
    pub fn handle_touch_event(
        event: &Event,
        width: u32,
        height: u32,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        let touches = Self::get_touches();
        
        if let Event::Touch(te) = event {
            match te {
                TouchEvent::Down(dn) => {
                    let x = dn.x_transformed(width);
                    let y = dn.y_transformed(height);
                    
                    if let Some((_group, idx)) = layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.hit_test(x, width as i32, Some(active_layer.clone()), &[]) {
                        Self::handle_touch_down(idx, active_layer, layers, touches, dn.seat_slot(), uinput)?;
                    }
                },
                TouchEvent::Motion(mtn) => {
                    if !touches.contains_key(&mtn.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (layer, _group, idx) = Self::get_touch_slot(touches, mtn.seat_slot())?;
                    Self::handle_touch_motion(*idx, layer, layers, uinput)?;
                },
                TouchEvent::Up(up) => {
                    if !touches.contains_key(&up.seat_slot()) {
                        return Ok(());
                    }
                    
                    let (layer, _group, idx) = Self::get_touch_slot(touches, up.seat_slot())?;
                    Self::handle_touch_up(*idx, layer, layers, uinput)?;
                    touches.remove(&up.seat_slot());
                },
                _ => {}
            }
        }
        Ok(())
    }

    fn get_touch_slot<'a>(
        touches: &'a HashMap<u32, (LayerKey, &'static str, usize)>,
        slot: u32
    ) -> crate::MainResult<&'a (LayerKey, &'static str, usize)> {
        touches.get(&slot)
            .ok_or_else(|| crate::MainError::TouchSlotNotFound(slot))
    }

    /// Handles touch down events for flat buttons in non-LayerKeys1 layers (Fn keys, App Layer 2, App Layer 3)
    pub fn handle_touch_down(
        idx: usize,
        active_layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        touches: &mut HashMap<u32, (LayerKey, &'static str, usize)>,
        seat_slot: u32,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        // Ensure this is only called for non-LayerKeys1 layers
        if *active_layer == LayerKey::Media {
            return Ok(());
        }

        let button = &mut layers.get_mut(active_layer).ok_or(crate::MainError::LayerNotFound(*active_layer))?.buttons[idx];
        if button.action == Key::Unknown {
            return Ok(());
        }
        
        touches.insert(seat_slot, (active_layer.clone(), "flat", idx));
        button.set_active(uinput, true);
        
        Ok(())
    }

    /// Handles touch motion events for flat buttons in non-LayerKeys1 layers
    pub fn handle_touch_motion(
        idx: usize,
        layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        // Ensure this is only called for non-LayerKeys1 layers
        if *layer == LayerKey::Media {
            return Ok(());
        }

        let button = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.buttons[idx];
        if button.action == Key::Unknown {
            return Ok(());
        }
        
        button.set_active(uinput, true);
        
        Ok(())
    }

    /// Handles touch up events for flat buttons in non-LayerKeys1 layers
    pub fn handle_touch_up(
        idx: usize,
        layer: &LayerKey,
        layers: &mut HashMap<LayerKey, FunctionLayer>,
        uinput: &mut UInputHandle<std::fs::File>,
    ) -> crate::MainResult<()> {
        // Ensure this is only called for non-LayerKeys1 layers
        if *layer == LayerKey::Media {
            return Ok(());
        }

        let button = &mut layers.get_mut(layer).ok_or(crate::MainError::LayerNotFound(*layer))?.buttons[idx];
        if button.action == Key::Unknown {
            return Ok(());
        }
        
        button.set_active(uinput, false);
        
        Ok(())
    }
} 