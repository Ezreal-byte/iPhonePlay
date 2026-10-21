//! Low-level hooks never call Bluetooth. They only validate the foreground
//! owner, update the admission gate, and enqueue input on the worker channel.
use super::{Gate, Input, protocol};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::GetAsyncKeyState, WindowsAndMessaging::*},
};

struct Context {
    gate: Arc<Gate>,
    anchor: POINT,
}
thread_local! { static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) }; }

pub(super) fn foreground_matches(window: isize) -> bool {
    window != 0 && unsafe { GetForegroundWindow().0 as isize == window }
}

pub(super) struct HookThread {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HookThread {
    pub fn start(gate: Arc<Gate>) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("iphoneplay-input-hooks".into())
            .spawn(move || {
                let mut anchor = POINT::default();
                let _ = unsafe { GetCursorPos(&mut anchor) };
                CONTEXT.with(|context| {
                    *context.borrow_mut() = Some(Context {
                        gate: Arc::clone(&gate),
                        anchor,
                    })
                });
                let module = unsafe { GetModuleHandleW(None) }
                    .ok()
                    .map(|h| HINSTANCE(h.0));
                let keyboard =
                    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
                let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
                let (keyboard, mouse) = match (keyboard, mouse) {
                    (Ok(keyboard), Ok(mouse)) => {
                        let _ = ready_tx.send(Ok(()));
                        (keyboard, mouse)
                    }
                    (keyboard, mouse) => {
                        let error = keyboard
                            .as_ref()
                            .err()
                            .or_else(|| mouse.as_ref().err())
                            .map(ToString::to_string)
                            .unwrap_or_default();
                        if let Ok(hook) = keyboard {
                            let _ = unsafe { UnhookWindowsHookEx(hook) };
                        }
                        if let Ok(hook) = mouse {
                            let _ = unsafe { UnhookWindowsHookEx(hook) };
                        }
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                let mut msg = MSG::default();
                let mut was_active = false;
                let mut hide_calls = 0;
                while !stop_thread.load(Ordering::Acquire) && !gate.shutdown.load(Ordering::Acquire)
                {
                    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                        unsafe {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                    if gate.admits() && !foreground_matches(gate.window.load(Ordering::Acquire)) {
                        gate.release("窗口已失焦，键鼠已回到本机。");
                    }
                    let active = gate.admits();
                    if active && !was_active {
                        CONTEXT.with(|context| {
                            if let Some(context) = &mut *context.borrow_mut()
                                && let Ok(rect) = context.gate.video_rect.lock()
                                && let Some([x, y, width, height]) = *rect
                            {
                                context.anchor = POINT {
                                    x: x + width / 2,
                                    y: y + height / 2,
                                };
                                let _ = unsafe { SetCursorPos(context.anchor.x, context.anchor.y) };
                            }
                        });
                        loop {
                            hide_calls += 1;
                            if unsafe { ShowCursor(false) } < 0 || hide_calls >= 16 {
                                break;
                            }
                        }
                    }
                    if !active && was_active {
                        for _ in 0..hide_calls {
                            unsafe {
                                ShowCursor(true);
                            }
                        }
                        hide_calls = 0;
                    }
                    was_active = active;
                    thread::sleep(Duration::from_millis(4));
                }
                for _ in 0..hide_calls {
                    unsafe {
                        ShowCursor(true);
                    }
                }
                unsafe {
                    let _ = UnhookWindowsHookEx(keyboard);
                    let _ = UnhookWindowsHookEx(mouse);
                }
                CONTEXT.with(|context| *context.borrow_mut() = None);
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                Err(error.to_string())
            }
        }
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // Windows owns this structure for the duration of the hook callback.
    let data = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if data.flags.0 & LLKHF_INJECTED.0 != 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let handled = CONTEXT.with(|context| {
        let context = context.borrow();
        let Some(context) = context.as_ref() else {
            return false;
        };
        if !context.gate.admits() {
            return false;
        }
        if !foreground_matches(context.gate.window.load(Ordering::Acquire)) {
            context.gate.release("窗口已失焦，键鼠已回到本机。");
            return false;
        }
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        if data.vkCode == 0x78 {
            if down {
                context
                    .gate
                    .release("已按 F9 释放键鼠。再次控制需手动开启。");
            }
            return true;
        }
        if (data.vkCode == 0x09 && unsafe { GetAsyncKeyState(0x12) } < 0)
            || (data.vkCode == 0x1B && unsafe { GetAsyncKeyState(0x11) } < 0)
        {
            context.gate.release("切换到其他窗口，键鼠已释放。");
            return false;
        }
        let vk = data.vkCode;
        let is_modifier = matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B..=0x5C);
        let is_consumer = matches!(vk, 0xAD..=0xB3);
        if !is_modifier && !is_consumer && protocol::key_usage(vk).is_none() {
            return false;
        }
        context.gate.edge(Input::Key {
            vk,
            scan: data.scanCode,
            extended: data.flags.0 & LLKHF_EXTENDED.0 != 0,
            down,
        });
        true
    });
    if handled {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let data = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    if data.flags & LLMHF_INJECTED != 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let handled = CONTEXT.with(|context| {
        let context = context.borrow();
        let Some(context) = context.as_ref() else {
            return false;
        };
        if !context.gate.admits() {
            return false;
        }
        if !foreground_matches(context.gate.window.load(Ordering::Acquire)) {
            context.gate.release("窗口已失焦，键鼠已回到本机。");
            return false;
        }
        let input = match wparam.0 as u32 {
            WM_MOUSEMOVE => {
                let dx = data.pt.x - context.anchor.x;
                let dy = data.pt.y - context.anchor.y;
                // Permit our centering event even on drivers that omit the
                // injected flag for SetCursorPos. Never feed it back to HID.
                if dx == 0 && dy == 0 {
                    return false;
                }
                context.gate.motion(dx, dy);
                let _ = unsafe { SetCursorPos(context.anchor.x, context.anchor.y) };
                return true;
            }
            WM_LBUTTONDOWN => Input::Button {
                mask: 1,
                down: true,
            },
            WM_LBUTTONUP => Input::Button {
                mask: 1,
                down: false,
            },
            WM_RBUTTONDOWN => Input::Button {
                mask: 2,
                down: true,
            },
            WM_RBUTTONUP => Input::Button {
                mask: 2,
                down: false,
            },
            WM_MBUTTONDOWN => Input::Button {
                mask: 4,
                down: true,
            },
            WM_MBUTTONUP => Input::Button {
                mask: 4,
                down: false,
            },
            WM_XBUTTONDOWN | WM_XBUTTONUP => Input::Button {
                mask: if data.mouseData >> 16 == 1 { 8 } else { 16 },
                down: wparam.0 as u32 == WM_XBUTTONDOWN,
            },
            WM_MOUSEWHEEL => Input::Wheel((data.mouseData >> 16) as i16 as i32),
            _ => return false,
        };
        context.gate.edge(input);
        true
    });
    if handled {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}
