//! The coordinator is outside the replaceable app and owns its own native window.
#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void, CString};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}
    #[link(name = "Foundation", kind = "framework")]
    extern "C" {
        static NSDefaultRunLoopMode: Obj;
    }
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
        fn objc_allocateClassPair(superclass: Obj, name: *const c_char, extra_bytes: usize) -> Obj;
        fn objc_registerClassPair(cls: Obj);
        fn class_addMethod(
            cls: Obj,
            name: Obj,
            implementation: *const c_void,
            types: *const c_char,
        ) -> i8;
    }
    type Obj = *mut c_void;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    fn class(name: &str) -> Obj {
        unsafe { objc_getClass(CString::new(name).unwrap().as_ptr()) }
    }
    fn selector(name: &str) -> Obj {
        unsafe { sel_registerName(CString::new(name).unwrap().as_ptr()) }
    }
    unsafe fn object(receiver: Obj, message: &str) -> Obj {
        let call: unsafe extern "C" fn(Obj, Obj) -> Obj =
            std::mem::transmute(objc_msgSend as *const ());
        call(receiver, selector(message))
    }
    unsafe fn send_void(receiver: Obj, message: &str) {
        let call: unsafe extern "C" fn(Obj, Obj) = std::mem::transmute(objc_msgSend as *const ());
        call(receiver, selector(message));
    }
    unsafe fn send_object(receiver: Obj, message: &str, value: Obj) {
        let call: unsafe extern "C" fn(Obj, Obj, Obj) =
            std::mem::transmute(objc_msgSend as *const ());
        call(receiver, selector(message), value);
    }
    unsafe fn send_bool(receiver: Obj, message: &str, value: bool) {
        let call: unsafe extern "C" fn(Obj, Obj, i8) =
            std::mem::transmute(objc_msgSend as *const ());
        call(receiver, selector(message), value as i8);
    }
    struct AutoreleasePool(Obj);
    impl AutoreleasePool {
        unsafe fn new() -> Self {
            Self(object(object(class("NSAutoreleasePool"), "alloc"), "init"))
        }
    }
    impl Drop for AutoreleasePool {
        fn drop(&mut self) {
            unsafe { send_void(self.0, "release") }
        }
    }
    unsafe fn string(value: &str) -> Obj {
        let call: unsafe extern "C" fn(Obj, Obj, *const c_char) -> Obj =
            std::mem::transmute(objc_msgSend as *const ());
        let value = CString::new(value.replace('\0', " ")).unwrap();
        call(
            class("NSString"),
            selector("stringWithUTF8String:"),
            value.as_ptr(),
        )
    }
    static RECOVER_REQUESTED: AtomicBool = AtomicBool::new(false);
    extern "C" fn recover_action(_: Obj, _: Obj, _: Obj) {
        RECOVER_REQUESTED.store(true, Ordering::Release);
    }
    static DETAILS_REQUESTED: AtomicBool = AtomicBool::new(false);
    extern "C" fn details_action(_: Obj, _: Obj, _: Obj) {
        DETAILS_REQUESTED.store(true, Ordering::Release);
    }
    fn recovery_target() -> Result<Obj, String> {
        unsafe {
            let name = CString::new("HerdrUpdateCoordinatorRecoveryTarget").unwrap();
            let mut cls = objc_getClass(name.as_ptr());
            if cls.is_null() {
                cls = objc_allocateClassPair(class("NSObject"), name.as_ptr(), 0);
                if cls.is_null() {
                    return Err("cannot allocate AppKit updater action".into());
                }
                for (action, implementation) in [
                    ("recoverUpdate:", recover_action as *const c_void),
                    ("toggleUpdateDetails:", details_action as *const c_void),
                ] {
                    if class_addMethod(
                        cls,
                        selector(action),
                        implementation,
                        b"v@:@\0".as_ptr().cast(),
                    ) == 0
                    {
                        return Err("cannot register AppKit updater action".into());
                    }
                }
                objc_registerClassPair(cls);
            }
            Ok(object(object(cls, "alloc"), "init"))
        }
    }
    // The presentation is independent of durable operation.detail and CLI diagnostics.
    #[derive(Clone, Copy)]
    pub enum Phase {
        Preparing,
        Stopping,
        Installing,
        Validating,
        Starting,
        RestoringOriginal,
        Reconciling,
        Finalizing,
        Attention(Attention),
        UserStopped,
    }
    #[derive(Clone, Copy)]
    pub enum Attention {
        NotStarted,
        Ownership,
        Completion,
        Release,
        Interrupted,
        Recovery,
        OriginalUnchanged,
    }
    impl Phase {
        fn text(self, korean: bool) -> (&'static str, &'static str) {
            match (self, korean) {
                (Self::Preparing, true) => ("업데이트 준비 중", "기존 프로필과 설정은 유지됩니다."),
                (Self::Preparing, false) => (
                    "Preparing update",
                    "Your existing profile and preferences are preserved.",
                ),
                (Self::Stopping, true) => ("앱 종료 중", "준비된 앱 인스턴스만 종료합니다."),
                (Self::Stopping, false) => {
                    ("Stopping app", "Only the prepared app instance is stopped.")
                }
                (Self::Installing, true) => {
                    ("업데이트 설치 중", "기록된 설치 원본에서 설치합니다.")
                }
                (Self::Installing, false) => {
                    ("Installing update", "Installing from the recorded source.")
                }
                (Self::Validating, true) => ("설치 확인 중", "실제 설치된 앱과 원본을 확인합니다."),
                (Self::Validating, false) => (
                    "Verifying installation",
                    "Checking the installed app and its source.",
                ),
                (Self::Starting, true) => (
                    "업데이트된 앱 시작 중",
                    "기존 프로필로 시작하고 준비 상태를 확인합니다.",
                ),
                (Self::Starting, false) => (
                    "Starting updated app",
                    "Checking readiness on your existing profile.",
                ),
                (Self::RestoringOriginal, true) => (
                    "원래 앱 시작 중",
                    "기존 프로필로 원래 버전을 확인합니다. 업데이트는 아닙니다.",
                ),
                (Self::RestoringOriginal, false) => (
                    "Starting original app",
                    "Checking the original version on your profile; this is not an upgrade.",
                ),
                (Self::Reconciling, true) => (
                    "업데이트 상태 확인 중",
                    "설치를 반복하지 않고 안전한 상태를 확인합니다.",
                ),
                (Self::Reconciling, false) => (
                    "Checking update state",
                    "Checking safely without repeating installation.",
                ),
                (Self::Finalizing, true) => (
                    "마무리 중",
                    "앱 준비 상태를 확인했습니다. 기록을 저장합니다.",
                ),
                (Self::Finalizing, false) => {
                    ("Finalizing", "App readiness verified. Saving the result.")
                }
                (Self::Attention(Attention::NotStarted), true) => (
                    "업데이트를 시작하지 못했습니다",
                    "상세 내용을 확인하거나 업데이트 상태를 조회하세요.",
                ),
                (Self::Attention(Attention::NotStarted), false) => (
                    "Update could not start",
                    "Review details or check the update status.",
                ),
                (Self::Attention(Attention::Ownership), true) => (
                    "업데이트 창 인계 실패",
                    "기존 앱은 종료하지 않았습니다. 상세 내용을 확인하세요.",
                ),
                (Self::Attention(Attention::Ownership), false) => (
                    "Updater handoff failed",
                    "The original app was not stopped. Review details.",
                ),
                (Self::Attention(Attention::Completion), true) => (
                    "완료 기록을 저장하지 못했습니다",
                    "새 앱은 준비됐지만 완료 확인이 필요합니다.",
                ),
                (Self::Attention(Attention::Completion), false) => (
                    "Could not save completion",
                    "The new app is ready, but completion needs review.",
                ),
                (Self::Attention(Attention::Release), true) => (
                    "예약 해제 실패",
                    "새 앱은 준비됐지만 복구 확인이 필요합니다.",
                ),
                (Self::Attention(Attention::Release), false) => (
                    "Could not release reservation",
                    "The new app is ready, but recovery needs review.",
                ),
                (Self::Attention(Attention::Interrupted), true) => (
                    "업데이트 확인 필요",
                    "자동 재시도나 되돌리기는 하지 않습니다. 상세 내용을 확인하세요.",
                ),
                (Self::Attention(Attention::Interrupted), false) => (
                    "Update needs attention",
                    "No automatic retry or rollback. Review details.",
                ),
                (Self::Attention(Attention::Recovery), true) => (
                    "복구 확인 필요",
                    "예약을 유지했습니다. 상세 내용을 확인하세요.",
                ),
                (Self::Attention(Attention::Recovery), false) => (
                    "Recovery needs attention",
                    "The reservation is retained. Review details.",
                ),
                (Self::Attention(Attention::OriginalUnchanged), true) => (
                    "업데이트가 적용되지 않았습니다",
                    "원래 앱이 유지되거나 복원됐습니다. 새 버전은 적용되지 않았습니다.",
                ),
                (Self::Attention(Attention::OriginalUnchanged), false) => (
                    "Update was not applied",
                    "The original app remains or was restored; this is not an upgrade.",
                ),
                (Self::UserStopped, true) => (
                    "사용자가 앱을 종료했습니다",
                    "앱을 자동으로 다시 시작하지 않습니다.",
                ),
                (Self::UserStopped, false) => (
                    "App stopped by user",
                    "The app will not be restarted automatically.",
                ),
            }
        }
        fn busy(self) -> bool {
            matches!(
                self,
                Self::Preparing
                    | Self::Stopping
                    | Self::Installing
                    | Self::Validating
                    | Self::Starting
                    | Self::RestoringOriginal
                    | Self::Reconciling
                    | Self::Finalizing
            )
        }
    }
    // The window is nonmodal: closing it only hides progress, never cancels the installer.
    pub struct Panel {
        app: Obj,
        window: Obj,
        heading: Obj,
        footer: Obj,
        explanation: Obj,
        progress: Obj,
        diagnostics: Obj,
        details_button: Obj,
        recovery_button: Obj,
        target: Obj,
        korean: bool,
        expanded: std::cell::Cell<bool>,
    }
    impl Panel {
        pub fn new(locale: &str) -> Result<Self, String> {
            unsafe {
                let _pool = AutoreleasePool::new();
                let korean = locale.starts_with("ko");
                let app = object(class("NSApplication"), "sharedApplication");
                if app.is_null() {
                    return Err("AppKit application unavailable".into());
                }
                let policy: unsafe extern "C" fn(Obj, Obj, isize) -> i8 =
                    std::mem::transmute(objc_msgSend as *const ());
                policy(app, selector("setActivationPolicy:"), 1);
                send_void(app, "finishLaunching");
                let init_window: unsafe extern "C" fn(Obj, Obj, Rect, usize, usize, i8) -> Obj =
                    std::mem::transmute(objc_msgSend as *const ());
                let window = init_window(
                    object(class("NSPanel"), "alloc"),
                    selector("initWithContentRect:styleMask:backing:defer:"),
                    Rect {
                        origin: Point { x: 0., y: 0. },
                        size: Size {
                            width: 510.,
                            height: 170.,
                        },
                    },
                    1 | 2 | 128,
                    2,
                    0,
                );
                if window.is_null() {
                    return Err("AppKit window unavailable".into());
                }
                send_bool(window, "setReleasedWhenClosed:", false);
                send_object(
                    window,
                    "setTitle:",
                    string(if korean {
                        "Herdr 앱 업데이트"
                    } else {
                        "Herdr app update"
                    }),
                );
                let content = object(window, "contentView");
                let init_view: unsafe extern "C" fn(Obj, Obj, Rect) -> Obj =
                    std::mem::transmute(objc_msgSend as *const ());
                let field = |y, height| {
                    let view = init_view(
                        object(class("NSTextField"), "alloc"),
                        selector("initWithFrame:"),
                        Rect {
                            origin: Point { x: 22., y },
                            size: Size {
                                width: 465.,
                                height,
                            },
                        },
                    );
                    send_bool(view, "setEditable:", false);
                    send_bool(view, "setSelectable:", true);
                    send_bool(view, "setBezeled:", false);
                    send_bool(view, "setDrawsBackground:", false);
                    send_object(content, "addSubview:", view);
                    view
                };
                let heading = field(119., 27.);
                let explanation = field(89., 25.);
                let footer = field(51., 16.);
                send_object(
                    footer,
                    "setStringValue:",
                    string(if korean {
                        "창을 닫아도 취소되지 않으며 프로필은 유지됩니다."
                    } else {
                        "Closing only hides this window; your profile is preserved."
                    }),
                );
                let progress = init_view(
                    object(class("NSProgressIndicator"), "alloc"),
                    selector("initWithFrame:"),
                    Rect {
                        origin: Point { x: 22., y: 68. },
                        size: Size {
                            width: 14.,
                            height: 14.,
                        },
                    },
                );
                send_bool(progress, "setIndeterminate:", true);
                let set_style: unsafe extern "C" fn(Obj, Obj, isize) =
                    std::mem::transmute(objc_msgSend as *const ());
                set_style(progress, selector("setStyle:"), 1); // NSProgressIndicatorSpinningStyle
                send_bool(progress, "setUsesThreadedAnimation:", true);
                send_bool(progress, "setDisplayedWhenStopped:", false);
                send_object(content, "addSubview:", progress);
                let target = recovery_target()?;
                let button = |x, width, action, title: &str| {
                    let view = init_view(
                        object(class("NSButton"), "alloc"),
                        selector("initWithFrame:"),
                        Rect {
                            origin: Point { x, y: 18. },
                            size: Size { width, height: 33. },
                        },
                    );
                    send_object(view, "setTitle:", string(title));
                    send_object(view, "setTarget:", target);
                    send_object(view, "setAction:", selector(action));
                    send_bool(view, "setHidden:", true);
                    send_object(content, "addSubview:", view);
                    view
                };
                let details_button = button(
                    22.,
                    175.,
                    "toggleUpdateDetails:",
                    if korean {
                        "진단 정보 보기"
                    } else {
                        "Show diagnostics"
                    },
                );
                let recovery_button = button(
                    286.,
                    200.,
                    "recoverUpdate:",
                    if korean {
                        "확인 후 다시 시작"
                    } else {
                        "Recover verified start"
                    },
                );
                let diagnostics = init_view(
                    object(class("NSScrollView"), "alloc"),
                    selector("initWithFrame:"),
                    Rect {
                        origin: Point { x: 22., y: 89. },
                        size: Size {
                            width: 465.,
                            height: 170.,
                        },
                    },
                );
                let text = init_view(
                    object(class("NSTextView"), "alloc"),
                    selector("initWithFrame:"),
                    Rect {
                        origin: Point { x: 0., y: 0. },
                        size: Size {
                            width: 465.,
                            height: 170.,
                        },
                    },
                );
                send_bool(text, "setEditable:", false);
                send_bool(text, "setSelectable:", true);
                send_object(diagnostics, "setDocumentView:", text);
                send_bool(diagnostics, "setHasVerticalScroller:", true);
                send_bool(diagnostics, "setHidden:", true);
                send_object(content, "addSubview:", diagnostics);
                // The scroll view retains its document view.
                send_void(text, "release");
                RECOVER_REQUESTED.store(false, Ordering::Release);
                DETAILS_REQUESTED.store(false, Ordering::Release);
                let panel = Self {
                    app,
                    window,
                    heading,
                    explanation,
                    footer,
                    progress,
                    diagnostics,
                    details_button,
                    recovery_button,
                    target,
                    korean,
                    expanded: std::cell::Cell::new(false),
                };
                panel.show(Phase::Preparing, None);
                send_void(window, "center");
                send_void(window, "orderFrontRegardless");
                panel.pump();
                Ok(panel)
            }
        }
        pub fn activate(&self) {
            unsafe {
                send_bool(self.app, "activateIgnoringOtherApps:", true);
                send_object(self.window, "makeKeyAndOrderFront:", std::ptr::null_mut());
            }
            self.pump();
        }
        pub fn show(&self, phase: Phase, diagnostic: Option<&str>) {
            unsafe {
                let _pool = AutoreleasePool::new();
                RECOVER_REQUESTED.store(false, Ordering::Release);
                send_bool(self.recovery_button, "setHidden:", true);
                let (heading, explanation) = phase.text(self.korean);
                send_object(self.heading, "setStringValue:", string(heading));
                send_object(self.explanation, "setStringValue:", string(explanation));
                if phase.busy() {
                    send_object(self.progress, "startAnimation:", std::ptr::null_mut());
                } else {
                    send_object(self.progress, "stopAnimation:", std::ptr::null_mut());
                }
                send_bool(self.details_button, "setHidden:", diagnostic.is_none());
                let document = object(self.diagnostics, "documentView");
                send_object(document, "setString:", string(diagnostic.unwrap_or("")));
                if diagnostic.is_none() && self.expanded.get() {
                    self.toggle_details();
                }
            }
            self.pump();
        }
        fn toggle_details(&self) {
            unsafe {
                let expanded = !self.expanded.get();
                self.expanded.set(expanded);
                let _pool = AutoreleasePool::new();
                let resize: unsafe extern "C" fn(Obj, Obj, Size) =
                    std::mem::transmute(objc_msgSend as *const ());
                resize(
                    self.window,
                    selector("setContentSize:"),
                    Size {
                        width: 510.,
                        height: if expanded { 365. } else { 170. },
                    },
                );
                let move_view: unsafe extern "C" fn(Obj, Obj, Point) =
                    std::mem::transmute(objc_msgSend as *const ());
                let offset = if expanded { 195. } else { 0. };
                move_view(
                    self.heading,
                    selector("setFrameOrigin:"),
                    Point {
                        x: 22.,
                        y: 119. + offset,
                    },
                );
                move_view(
                    self.explanation,
                    selector("setFrameOrigin:"),
                    Point {
                        x: 22.,
                        y: 89. + offset,
                    },
                );
                send_bool(self.diagnostics, "setHidden:", !expanded);
                send_object(
                    self.details_button,
                    "setTitle:",
                    string(match (self.korean, expanded) {
                        (true, true) => "진단 정보 숨기기",
                        (true, false) => "진단 정보 보기",
                        (false, true) => "Hide diagnostics",
                        (false, false) => "Show diagnostics",
                    }),
                );
            }
        }
        pub fn offer_recovery(&self) {
            RECOVER_REQUESTED.store(false, Ordering::Release);
            unsafe {
                send_bool(self.recovery_button, "setHidden:", false);
            }
            self.pump();
        }
        pub fn recovery_requested(&self) -> bool {
            if RECOVER_REQUESTED.swap(false, Ordering::AcqRel) {
                unsafe {
                    send_bool(self.recovery_button, "setHidden:", true);
                }
                true
            } else {
                false
            }
        }
        pub fn close(&self) {
            unsafe {
                send_object(self.window, "orderOut:", std::ptr::null_mut());
            }
        }
        pub fn pump(&self) {
            unsafe {
                let _pool = AutoreleasePool::new();
                let date = object(class("NSDate"), "distantPast");
                let next: unsafe extern "C" fn(Obj, Obj, usize, Obj, Obj, i8) -> Obj =
                    std::mem::transmute(objc_msgSend as *const ());
                for _ in 0..32 {
                    let event = next(
                        self.app,
                        selector("nextEventMatchingMask:untilDate:inMode:dequeue:"),
                        usize::MAX,
                        date,
                        NSDefaultRunLoopMode,
                        1,
                    );
                    if event.is_null() {
                        break;
                    }
                    send_object(self.app, "sendEvent:", event);
                }
                if DETAILS_REQUESTED.swap(false, Ordering::AcqRel) {
                    self.toggle_details();
                }
                send_void(self.app, "updateWindows");
            }
        }
        pub fn visible(&self) -> bool {
            unsafe {
                let visible: unsafe extern "C" fn(Obj, Obj) -> i8 =
                    std::mem::transmute(objc_msgSend as *const ());
                visible(self.window, selector("isVisible")) != 0
            }
        }
    }
    impl Drop for Panel {
        fn drop(&mut self) {
            unsafe {
                send_object(self.window, "orderOut:", std::ptr::null_mut());
                for view in [
                    self.heading,
                    self.explanation,
                    self.footer,
                    self.progress,
                    self.diagnostics,
                    self.details_button,
                    self.recovery_button,
                    self.target,
                    self.window,
                ] {
                    send_void(view, "release");
                }
            }
        }
    }
    pub use Panel as NativePanel;
}
#[cfg(target_os = "macos")]
pub use mac::{Attention, NativePanel as Panel, Phase};
#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
pub enum Attention {
    NotStarted,
    Ownership,
    Completion,
    Release,
    Interrupted,
    Recovery,
    OriginalUnchanged,
}
#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
pub enum Phase {
    Preparing,
    Stopping,
    Installing,
    Validating,
    Starting,
    RestoringOriginal,
    Reconciling,
    Finalizing,
    Attention(Attention),
    UserStopped,
}
#[cfg(not(target_os = "macos"))]
pub struct Panel;
#[cfg(not(target_os = "macos"))]
impl Panel {
    pub fn new(_: &str) -> Result<Self, String> {
        Err("native updater requires macOS AppKit".into())
    }
    pub fn activate(&self) {}
    pub fn show(&self, _: Phase, _: Option<&str>) {}
    pub fn pump(&self) {}
    pub fn visible(&self) -> bool {
        false
    }
    pub fn offer_recovery(&self) {}
    pub fn recovery_requested(&self) -> bool {
        false
    }
    pub fn close(&self) {}
}
