use crate::bubble::BubblePlacement;
use crate::i18n::{text, LanguagePreference, Message, UiLocale};
use crate::lifecycle::LifecycleSettings;
use crate::lifecycle_settings_ui::LifecycleSettingsCard;
use crate::preferences::{BubbleAppearance, BubbleColor, BubblePalette, BubbleTheme, MenuBarMode};
use crate::sources::ObservationPreferences;
use crate::state::Scene;
use crate::ui::MenuTarget;
use crate::update_card::{AppUpdateCard, UpdateCardModel};
use desktop_pet_contracts::ObservationSelection;
use desktop_pet_contracts::{HostConnections, LifecycleSettingView};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly,
    Message as ObjcMessage,
};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameDarkAqua, NSBackingStoreType, NSBezelStyle, NSBezierPath, NSBox,
    NSBoxType, NSButton, NSButtonType, NSCellImagePosition, NSColor, NSControl, NSControlSize,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags,
    NSFloatingWindowLevel, NSFont, NSImage, NSImageScaling, NSImageView, NSPanel, NSPopUpButton,
    NSScreen, NSScrollElasticity, NSScrollView, NSScroller, NSScrollerStyle, NSSwitch, NSText,
    NSTextAlignment, NSTextField, NSTextView, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};

use objc2_foundation::{
    NSArray, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString, NSUndoManager, NSValue,
};
use std::cell::{Cell, OnceCell};
use std::fmt::Write as _;
const PANEL_WIDTH: f64 = 352.0;
const PANEL_HEIGHT: f64 = 540.0;
const PANEL_EDGE_INSET: f64 = 10.0;
const PANEL_RADIUS: f64 = 16.0;
const CARD_RADIUS: f64 = 10.0;
const SCROLL_TOP: f64 = 86.0;
const FOOTER_HEIGHT: f64 = 50.0;
const INITIAL_CHARACTER_HEIGHT: f64 = 72.0;
const MIN_SCALE: f64 = 0.35;
const MAX_SCALE: f64 = 1.25;
const DETAIL_PAGE_COUNT: usize = 7;
const DETAIL_MIN_WIDTH: f64 = 360.0;
const DETAIL_MIN_HEIGHT: f64 = 96.0;
const DETAIL_DEFAULT_WIDTH: f64 = 600.0;
const DETAIL_DEFAULT_MAX_HEIGHT: f64 = 560.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum SettingsTab {
    Appearance = 0,
    Connections = 1,
    App = 2,
}

impl SettingsTab {
    pub(crate) fn from_tag(tag: isize) -> Option<Self> {
        match tag {
            0 => Some(Self::Appearance),
            1 => Some(Self::Connections),
            2 => Some(Self::App),
            _ => None,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum SettingsDetailPage {
    Appearance = 0,
    Connections = 1,
    MenuBar = 2,
    UpdateSettings = 3,
    ConnectionDiagnostics = 4,
    UpdateDetails = 5,
    AppRecovery = 6,
}

impl SettingsDetailPage {
    pub(crate) fn from_tag(tag: isize) -> Option<Self> {
        match tag {
            0 => Some(Self::Appearance),
            1 => Some(Self::Connections),
            2 => Some(Self::MenuBar),
            3 => Some(Self::UpdateSettings),
            4 => Some(Self::ConnectionDiagnostics),
            5 => Some(Self::UpdateDetails),
            6 => Some(Self::AppRecovery),
            _ => None,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
    fn message(self) -> Message {
        match self {
            Self::Appearance => Message::AppearanceDetails,
            Self::Connections => Message::ConnectionSettings,
            Self::MenuBar => Message::MenuBarSettings,
            Self::UpdateSettings => Message::UpdateSettings,
            Self::ConnectionDiagnostics => Message::ConnectionDiagnostics,
            Self::UpdateDetails => Message::UpdateDetails,
            Self::AppRecovery => Message::AppRecovery,
        }
    }
}

#[derive(Default)]
struct SettingsDetailWindowIvars {
    composition_escape: Cell<Option<u64>>,
    update_frozen: Cell<bool>,
    field_editor: OnceCell<Retained<SettingsFieldEditor>>,
}

define_class!(
    #[unsafe(super = NSWindow)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetSettingsDetailWindow"]
    #[ivars = SettingsDetailWindowIvars]
    struct SettingsDetailNativeWindow;
    unsafe impl NSObjectProtocol for SettingsDetailNativeWindow {}
    impl SettingsDetailNativeWindow {
        #[unsafe(method_id(fieldEditor:forObject:))]
        fn field_editor(&self, create: bool, _object: Option<&AnyObject>) -> Option<Retained<NSText>> {
            if create {
                self.ivars().field_editor.get_or_init(|| {
                    let editor = SettingsFieldEditor::new(self.mtm());
                    editor.ivars().set(self.ivars().update_frozen.get());
                    editor
                });
            }
            self.ivars().field_editor.get().map(|editor| editor.retain().into_super().into_super())
        }
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            let composing_escape = event.r#type() == objc2_app_kit::NSEventType::KeyDown && event.keyCode() == 53 && self.firstResponder()
                .and_then(|responder| responder.downcast::<NSTextView>().ok())
                .is_some_and(|editor| unsafe { msg_send![&*editor, hasMarkedText] });
            self.ivars().composition_escape.set(composing_escape.then(|| event.timestamp().to_bits()));
            let _: () = unsafe { msg_send![super(self), sendEvent: event] };
        }
        #[unsafe(method(close))]
        fn close(&self) { crate::ui::request_settings_detail_close(); }
        #[unsafe(method(performClose:))]
        fn perform_close(&self, _sender: Option<&AnyObject>) { crate::ui::request_settings_detail_close(); }
        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, sender: Option<&AnyObject>) {
            if self.ivars().composition_escape.get().is_some_and(|number| {
                objc2_app_kit::NSApplication::sharedApplication(self.mtm()).currentEvent()
                    .is_some_and(|event| event.timestamp().to_bits() == number)
            }) { return; }
            if let Some(editor) = self.firstResponder().and_then(|r| r.downcast::<NSTextView>().ok()) {
                let marked: bool = unsafe { msg_send![&*editor, hasMarkedText] };
                if marked {
                    let _: () = unsafe { msg_send![&*editor, cancelOperation: sender] };
                    return;
                }
            }
            crate::ui::request_settings_detail_close();
        }
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            if event.keyCode() == 13 && event.modifierFlags().contains(NSEventModifierFlags::Command) {
                crate::ui::request_settings_detail_close();
                true
            } else if self.handle_edit_shortcut(event) {
                true
            } else {
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }
        }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if self.ivars().composition_escape.get() == Some(event.timestamp().to_bits()) {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
                return;
            }
            if self.handle_edit_shortcut(event) {
                return;
            }
            if event.keyCode() == 53 {
                if let Some(editor) = self.firstResponder().and_then(|r| r.downcast::<NSTextView>().ok()) {
                    let marked: bool = unsafe { msg_send![&*editor, hasMarkedText] };
                    if marked {
                        let _: () = unsafe { msg_send![&*editor, keyDown: event] };
                        return;
                    }
                }
                crate::ui::request_settings_detail_close();
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }
    }
);

define_class!(
    #[unsafe(super = NSTextField)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetSettingsColorField"]
    #[ivars = Cell<bool>]
    struct SettingsColorNativeField;
    unsafe impl NSObjectProtocol for SettingsColorNativeField {}
    impl SettingsColorNativeField {
        #[unsafe(method(isEnabled))]
        fn is_enabled(&self) -> bool {
            !self.ivars().get() && unsafe { msg_send![super(self), isEnabled] }
        }
    }
);

// Changing NSTextView's editable state discards active marked text. A native
// field editor keeps that session intact and rejects mutations at their source.
define_class!(
    #[unsafe(super = NSTextView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetSettingsFieldEditor"]
    #[ivars = Cell<bool>]
    struct SettingsFieldEditor;
    unsafe impl NSObjectProtocol for SettingsFieldEditor {}
    impl SettingsFieldEditor {
        #[unsafe(method(setAccessibilityValue:))]
        fn set_accessibility_value(&self, value: Option<&AnyObject>) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), setAccessibilityValue: value] };
            }
        }
        #[unsafe(method(setAccessibilitySelectedText:))]
        fn set_accessibility_selected_text(&self, value: Option<&NSString>) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), setAccessibilitySelectedText: value] };
            }
        }
        #[unsafe(method(setAccessibilitySelectedTextRange:))]
        fn set_accessibility_selected_text_range(&self, range: NSRange) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), setAccessibilitySelectedTextRange: range] };
            }
        }
        #[unsafe(method(isEditable))]
        fn is_editable(&self) -> bool {
            !self.ivars().get() && unsafe { msg_send![super(self), isEditable] }
        }
        #[unsafe(method(shouldChangeTextInRange:replacementString:))]
        fn should_change_text(&self, range: NSRange, replacement: Option<&NSString>) -> bool {
            !self.ivars().get() && unsafe {
                msg_send![super(self), shouldChangeTextInRange: range, replacementString: replacement]
            }
        }
        #[unsafe(method(shouldChangeTextInRanges:replacementStrings:))]
        fn should_change_ranges(&self, ranges: &NSArray<NSValue>, replacements: Option<&NSArray<NSString>>) -> bool {
            !self.ivars().get() && unsafe {
                msg_send![super(self), shouldChangeTextInRanges: ranges, replacementStrings: replacements]
            }
        }
        #[unsafe(method(insertText:))]
        fn insert_text(&self, text: &AnyObject) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), insertText: text] };
            }
        }
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text_replacement(&self, text: &AnyObject, range: NSRange) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), insertText: text, replacementRange: range] };
            }
        }
        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(&self, text: &AnyObject, selected: NSRange, replacement: NSRange) {
            if !self.ivars().get() {
                let _: () = unsafe {
                    msg_send![super(self), setMarkedText: text, selectedRange: selected, replacementRange: replacement]
                };
            }
        }
        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), unmarkText] };
            }
        }
        #[unsafe(method(cut:))]
        fn cut(&self, sender: Option<&AnyObject>) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), cut: sender] };
            }
        }
        #[unsafe(method(paste:))]
        fn paste(&self, sender: Option<&AnyObject>) {
            if !self.ivars().get() {
                let _: () = unsafe { msg_send![super(self), paste: sender] };
            }
        }
        #[unsafe(method(undo:))]
        fn undo(&self, _sender: Option<&AnyObject>) {
            if !self.ivars().get() {
                if let Some(manager) = self.undoManager() {
                    if manager.canUndo() { manager.undo(); }
                }
            }
        }
        #[unsafe(method(redo:))]
        fn redo(&self, _sender: Option<&AnyObject>) {
            if !self.ivars().get() {
                if let Some(manager) = self.undoManager() {
                    if manager.canRedo() { manager.redo(); }
                }
            }
        }
    }
);

#[derive(Default)]
struct SettingsDetailRootIvars {
    layout_dirty: Cell<bool>,
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetSettingsDetailRoot"]
    #[ivars = SettingsDetailRootIvars]
    struct SettingsDetailRoot;
    unsafe impl NSObjectProtocol for SettingsDetailRoot {}
    impl SettingsDetailRoot {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.ivars().layout_dirty.set(true);
        }
    }
);

#[derive(Default)]
struct BubbleColorDraft {
    latest: Option<BubbleAppearance>,
    baseline: Option<BubbleAppearance>,
}

impl BubbleColorDraft {
    fn receive(&mut self, appearance: BubbleAppearance, protected: bool) {
        let already_conflicted = self.conflicted();
        self.latest = Some(appearance);
        if self.baseline.is_none() || (!protected && !already_conflicted) {
            self.baseline = Some(appearance);
        }
    }

    fn saved(&mut self, appearance: BubbleAppearance) {
        self.latest = Some(appearance);
        self.baseline = Some(appearance);
    }

    fn conflicted(&self) -> bool {
        self.latest.is_some() && self.latest != self.baseline
    }

    fn rebase(&mut self) -> bool {
        let Some(latest) = self.latest else {
            return false;
        };
        self.baseline = Some(latest);
        true
    }
}

const BG_RED: f64 = 0.114;
const BG_GREEN: f64 = 0.114;
const BG_BLUE: f64 = 0.125;
const CARD_RED: f64 = 0.161;
const CARD_GREEN: f64 = 0.161;
const CARD_BLUE: f64 = 0.176;
const CARD_BORDER_RED: f64 = 0.28;
const CARD_BORDER_GREEN: f64 = 0.28;
const CARD_BORDER_BLUE: f64 = 0.32;
const CARD_BORDER_ALPHA: f64 = 0.50;
const TRACK_RED: f64 = 0.082;
const TRACK_GREEN: f64 = 0.082;
const TRACK_BLUE: f64 = 0.094;
const PRIMARY_RED: f64 = 0.95;
const PRIMARY_GREEN: f64 = 0.95;
const PRIMARY_BLUE: f64 = 0.97;
const SECONDARY_RED: f64 = 0.60;
const SECONDARY_GREEN: f64 = 0.61;
const SECONDARY_BLUE: f64 = 0.66;

define_class!(
    // SAFETY:
    // - MenuPanelWindow is created and used only on AppKit's main thread.
    // - It owns no Rust references and forwards normal responder messages to NSPanel.
    #[unsafe(super = NSPanel)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetMenuPanel"]
    struct MenuPanelWindow;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for MenuPanelWindow {}

    impl MenuPanelWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if event.keyCode() == 53 {
                self.resignKeyWindow();
                self.orderOut(None);
                return;
            }
            if event.keyCode() == 48 {
                if event
                    .modifierFlags()
                    .contains(NSEventModifierFlags::Shift)
                {
                    self.selectPreviousKeyView(None);
                } else {
                    self.selectNextKeyView(None);
                }
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, sender: Option<&AnyObject>) {
            self.resignKeyWindow();
            self.orderOut(None);
            let _ = sender;
        }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key_window(&self) {
            // Keep the panel ordered while AppKit changes key windows. A
            // status-item click can resign this panel before dispatching its
            // toggle action; hiding here would turn a close click into reopen.
            let _: () = unsafe { msg_send![super(self), resignKeyWindow] };
        }
    }
);

define_class!(
    // SAFETY: MenuPanelRoot is main-thread-only and has no Rust-owned ivars.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetMenuPanelRoot"]
    struct MenuPanelRoot;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for MenuPanelRoot {}

    impl MenuPanelRoot {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let bounds = self.bounds();
            let fill = color(BG_RED, BG_GREEN, BG_BLUE, 1.0);
            fill.setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                bounds,
                PANEL_RADIUS,
                PANEL_RADIUS,
            )
            .fill();

            let border = color(CARD_BORDER_RED, CARD_BORDER_GREEN, CARD_BORDER_BLUE, 0.45);
            border.setStroke();
            let border_bounds = inset_rect(bounds, 0.5);
            let border_path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                border_bounds,
                (PANEL_RADIUS - 0.5).max(0.0),
                (PANEL_RADIUS - 0.5).max(0.0),
            );
            border_path.setLineWidth(1.0);
            border_path.stroke();

            let highlight = color(1.0, 1.0, 1.0, 0.08);
            highlight.setStroke();
            let highlight_path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                inset_rect(bounds, 1.0),
                (PANEL_RADIUS - 1.0).max(0.0),
                (PANEL_RADIUS - 1.0).max(0.0),
            );
            highlight_path.setLineWidth(1.0);
            highlight_path.stroke();

            // Footer separator hairline
            let sep_y = bounds.size.height - FOOTER_HEIGHT;
            if sep_y > 0.0 {
                let sep = color(CARD_BORDER_RED, CARD_BORDER_GREEN, CARD_BORDER_BLUE, 0.35);
                sep.setStroke();
                let sep_path = NSBezierPath::bezierPath();
                sep_path.moveToPoint(NSPoint::new(14.0, sep_y));
                sep_path.lineToPoint(NSPoint::new((bounds.size.width - 14.0).max(14.0), sep_y));
                sep_path.setLineWidth(1.0);
                sep_path.stroke();
            }
        }
    }
);

define_class!(
    // SAFETY: MenuPanelDocument is a main-thread-only flipped document view.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetMenuPanelDocument"]
    struct MenuPanelDocument;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for MenuPanelDocument {}

    impl MenuPanelDocument {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

define_class!(
    // SAFETY: MenuPanelCard is a main-thread-only decorative view.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetMenuPanelCard"]
    struct MenuPanelCard;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for MenuPanelCard {}

    impl MenuPanelCard {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let bounds = self.bounds();
            let fill = color(CARD_RED, CARD_GREEN, CARD_BLUE, 1.0);
            fill.setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                bounds,
                CARD_RADIUS,
                CARD_RADIUS,
            )
            .fill();

            let border = color(
                CARD_BORDER_RED,
                CARD_BORDER_GREEN,
                CARD_BORDER_BLUE,
                CARD_BORDER_ALPHA,
            );
            border.setStroke();
            let border_path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                inset_rect(bounds, 0.5),
                (CARD_RADIUS - 0.5).max(0.0),
                (CARD_RADIUS - 0.5).max(0.0),
            );
            border_path.setLineWidth(1.0);
            border_path.stroke();
        }
    }
);

define_class!(
    // SAFETY: MenuTabTrack is a main-thread-only decorative view.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetMenuTabTrack"]
    struct MenuTabTrack;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for MenuTabTrack {}

    impl MenuTabTrack {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let bounds = self.bounds();
            let fill = color(TRACK_RED, TRACK_GREEN, TRACK_BLUE, 1.0);
            fill.setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, 8.0, 8.0).fill();

            let border = color(CARD_BORDER_RED, CARD_BORDER_GREEN, CARD_BORDER_BLUE, 0.35);
            border.setStroke();
            let border_path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                inset_rect(bounds, 0.5),
                7.5,
                7.5,
            );
            border_path.setLineWidth(1.0);
            border_path.stroke();
        }
    }
);

impl MenuPanelWindow {
    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSPanel's designated initializer is inherited by this
        // borderless activating subclass.
        unsafe {
            msg_send![
                super(this),
                initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        }
    }
}

impl MenuPanelRoot {
    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSView's initWithFrame: has the expected signature.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

impl MenuPanelDocument {
    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSView's initWithFrame: has the expected signature.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

impl MenuPanelCard {
    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSView's initWithFrame: has the expected signature.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

impl MenuTabTrack {
    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSView's initWithFrame: has the expected signature.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

impl SettingsFieldEditor {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(false));
        let this: Retained<Self> =
            unsafe { msg_send![super(this), initWithFrame: NSRect::default()] };
        this.setFieldEditor(true);
        this.setRichText(false);
        this.setAllowsUndo(true);
        this
    }
}

impl SettingsDetailNativeWindow {
    fn handle_edit_shortcut(&self, event: &NSEvent) -> bool {
        let modifiers = event.modifierFlags()
            & (NSEventModifierFlags::Command
                | NSEventModifierFlags::Shift
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Option);
        let Some(key) = event.charactersIgnoringModifiers() else {
            return false;
        };
        if key.length() != 1 {
            return false;
        }
        let Ok(key) = u8::try_from(key.characterAtIndex(0)) else {
            return false;
        };
        if !key.is_ascii() {
            return false;
        }
        let key = key.to_ascii_lowercase();
        let command = NSEventModifierFlags::Command;
        let redo = modifiers == (command | NSEventModifierFlags::Shift);
        if !matches!(key, b'a' | b'c' | b'x' | b'v' | b'z')
            || (modifiers != command && !(key == b'z' && redo))
        {
            return false;
        }
        let Some(editor) = self
            .firstResponder()
            .and_then(|responder| responder.downcast::<NSTextView>().ok())
        else {
            return false;
        };
        if !editor.window().is_some_and(|window| {
            Retained::as_ptr(&window).cast::<()>() == (self as *const Self).cast::<()>()
        }) {
            return false;
        }
        if matches!(key, b'x' | b'v' | b'z')
            && (self.ivars().update_frozen.get() || !editor.isEditable())
        {
            return true;
        }
        let marked: bool = unsafe { msg_send![&*editor, hasMarkedText] };
        if marked {
            return false;
        }
        match key {
            b'a' => unsafe { editor.selectAll(None) },
            b'c' => unsafe { editor.copy(None) },
            b'x' => unsafe { editor.cut(None) },
            b'v' => unsafe { editor.paste(None) },
            b'z' => {
                let undo: Option<Retained<NSUndoManager>> =
                    unsafe { msg_send![&*editor, undoManager] };
                if let Some(undo) = undo {
                    if redo {
                        if undo.canRedo() {
                            undo.redo();
                        }
                    } else if undo.canUndo() {
                        undo.undo();
                    }
                }
            }
            _ => unreachable!(),
        }
        true
    }

    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SettingsDetailWindowIvars::default());
        unsafe {
            msg_send![super(this), initWithContentRect: frame,
            styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable,
            backing: NSBackingStoreType::Buffered, defer: false]
        }
    }
    fn finish_shutdown(&self) {
        self.orderOut(None);
        let _: () = unsafe { msg_send![super(self), close] };
    }
}

impl SettingsColorNativeField {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(false));
        unsafe { msg_send![super(this), initWithFrame: NSRect::default()] }
    }
}

impl SettingsDetailRoot {
    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SettingsDetailRootIvars::default());
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

struct ScrollReadingPosition {
    offset: f64,
    anchor: Option<(Retained<NSView>, f64)>,
}

pub(crate) struct MenuPanel {
    panel: Retained<MenuPanelWindow>,
    root: Retained<MenuPanelRoot>,
    tab_track: Retained<MenuTabTrack>,
    tab_indicator: Retained<NSBox>,
    scroll: Retained<NSScrollView>,
    document: Retained<MenuPanelDocument>,
    character_tab: Retained<MenuPanelDocument>,
    bubble_tab: Retained<MenuPanelDocument>,
    settings_tab: Retained<MenuPanelDocument>,
    detail_window: Retained<SettingsDetailNativeWindow>,
    detail_root: Retained<SettingsDetailRoot>,
    detail_scroll: Retained<NSScrollView>,
    detail_document: Retained<MenuPanelDocument>,
    detail_documents: [Retained<MenuPanelDocument>; DETAIL_PAGE_COUNT],
    selected_detail: Option<SettingsDetailPage>,
    detail_offsets: Cell<[f64; DETAIL_PAGE_COUNT]>,
    detail_user_sizes: Cell<[Option<NSSize>; DETAIL_PAGE_COUNT]>,
    detail_layout_size: Cell<Option<NSSize>>,
    detail_openers: [Retained<NSButton>; DETAIL_PAGE_COUNT],
    detail_opener: Option<Retained<NSButton>>,
    saved_color_selection: Option<(usize, NSRange)>,
    character_card: Retained<MenuPanelCard>,
    bubble_visibility_card: Retained<MenuPanelCard>,
    bubble_theme_card: Retained<MenuPanelCard>,
    appearance_card: Retained<MenuPanelCard>,
    scale_card: Retained<MenuPanelCard>,
    app_general_card: Retained<MenuPanelCard>,
    appearance_error: Retained<NSTextField>,
    lifecycle_error_detail: Retained<NSTextField>,
    connection_summary: Retained<NSTextField>,
    connection_selection_summary: Retained<NSTextField>,
    connection_error_summary: Retained<NSTextField>,
    connection_settings_error: Retained<NSTextField>,
    connection_snapshot_unavailable: bool,
    connection_query_error: Option<String>,
    connection_operation_error: Option<String>,
    observation_locale: Option<UiLocale>,
    footer_details: Retained<NSButton>,
    update_frozen: bool,
    reset_settings_busy: bool,
    reset_settings: Retained<NSButton>,
    recovery_reset_help: Retained<NSTextField>,
    recovery_preserved_help: Retained<NSTextField>,
    settings_recovery_status: Retained<NSTextField>,
    update_details_problem: bool,
    control_refresh_depth: usize,
    tabs: [Retained<NSButton>; 3],
    placement_buttons: [Retained<NSButton>; 5],
    character_view: Retained<NSView>,
    character_content_height: f64,
    character_editing_open: Retained<NSButton>,
    character_browser_open: Retained<NSButton>,
    target: Retained<MenuTarget>,
    title: Retained<NSTextField>,
    panel_title: Retained<NSTextField>,
    character_visible_label: Retained<NSTextField>,
    character_visible_switch: Retained<NSSwitch>,
    bubble_visible_label: Retained<NSTextField>,
    bubble_visible_switch: Retained<NSSwitch>,
    status_indicators_label: Retained<NSTextField>,
    status_indicators_switch: Retained<NSSwitch>,
    bubble_placement_label: Retained<NSTextField>,
    bubble_theme_label: Retained<NSTextField>,
    bubble_theme_popup: Retained<NSPopUpButton>,
    bubble_colors_label: Retained<NSTextField>,
    bubble_color_labels: [Retained<NSTextField>; 5],
    bubble_color_fields: [Retained<SettingsColorNativeField>; 5],
    bubble_color_swatches: [Retained<NSBox>; 5],
    bubble_apply: Retained<NSButton>,
    bubble_reset: Retained<NSButton>,
    bubble_reload: Retained<NSButton>,
    bubble_rebase: Retained<NSButton>,
    bubble_draft: BubbleColorDraft,
    bubble_control_state: Option<(bool, bool)>,
    bubble_conflict_label: Option<bool>,
    settings_appearance_label: Retained<NSTextField>,
    click_behavior_label: Retained<NSTextField>,
    full_passthrough_label: Retained<NSTextField>,
    full_passthrough_help: Retained<NSTextField>,
    full_passthrough_switch: Retained<NSSwitch>,
    alpha_passthrough_label: Retained<NSTextField>,
    alpha_passthrough_help: Retained<NSTextField>,
    alpha_passthrough_switch: Retained<NSSwitch>,
    scale_label: Retained<NSTextField>,
    scale_readout: Retained<NSTextField>,
    scale_down: Retained<NSButton>,
    scale_up: Retained<NSButton>,
    language_label: Retained<NSTextField>,
    language_popup: Retained<NSPopUpButton>,
    menu_bar_card: Retained<MenuPanelCard>,
    menu_bar_label: Retained<NSTextField>,
    menu_bar_preview_well: Retained<NSBox>,
    menu_bar_preview: Retained<NSImageView>,
    menu_bar_separator: Retained<NSBox>,
    menu_bar_icon_status: Retained<NSTextField>,
    menu_bar_visibility: Retained<NSTextField>,
    menu_bar_popup: Retained<NSPopUpButton>,
    menu_bar_help: Retained<NSTextField>,
    menu_bar_choose: Retained<NSButton>,
    menu_bar_restore: Retained<NSButton>,
    menu_bar_icon_help: Retained<NSTextField>,
    menu_bar_icon_error: Retained<NSTextField>,
    menu_bar_icon_error_detail: Option<(Message, String)>,
    menu_bar_icon_custom: bool,
    menu_bar_icon_busy: Cell<bool>,
    lifecycle_card: LifecycleSettingsCard,
    frozen_controls: Vec<(Retained<NSControl>, bool)>,
    update_card: AppUpdateCard,
    observation_card: Retained<MenuPanelCard>,
    observation_title: Retained<NSTextField>,
    connection_host_label: Retained<NSTextField>,
    connection_host_popup: Retained<NSPopUpButton>,
    connection_status: Retained<NSTextField>,
    connection_auto_help: Retained<NSTextField>,
    connection_profile_title: Retained<NSTextField>,
    connection_profile_labels: [Retained<NSTextField>; 5],
    connection_profile_values: [Retained<NSTextField>; 5],
    connection_hosts: HostConnections,
    provider_labels: std::collections::BTreeMap<String, String>,
    provider_ids: Vec<String>,
    observation_error: Retained<NSTextField>,
    observation_preferences: Option<ObservationPreferences>,
    locale: UiLocale,
    status: Retained<NSTextField>,
    presentation_error: Option<String>,
    status_text: String,
    reset: Retained<NSButton>,
    restart: Retained<NSButton>,
    quit: Retained<NSButton>,
    selected_tab: SettingsTab,
    tab_offsets: Cell<[f64; 3]>,
}

impl MenuPanel {
    pub(crate) fn new(
        target: &MenuTarget,
        character_view: &NSView,
        locale: UiLocale,
        mtm: MainThreadMarker,
    ) -> Self {
        let initial_height = PANEL_HEIGHT;
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(PANEL_WIDTH, initial_height),
        );
        let panel = MenuPanelWindow::new(frame, mtm);
        configure_panel(&panel);
        if let Some(appearance) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua })
        {
            let _: () = unsafe { msg_send![&*panel, setAppearance: Some(&*appearance)] };
        }

        let root = MenuPanelRoot::new(frame, mtm);
        set_accessibility_element(&*root, false);
        root.setAutoresizingMask(
            objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        panel.setContentView(Some(&root));

        let document = MenuPanelDocument::new(
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(PANEL_WIDTH, PANEL_HEIGHT),
            ),
            mtm,
        );
        set_accessibility_element(&*document, false);
        document.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);

        let scroll: Retained<NSScrollView> = unsafe {
            msg_send![
                NSScrollView::alloc(mtm),
                initWithFrame: NSRect::new(
                    NSPoint::new(0.0, SCROLL_TOP),
                    NSSize::new(PANEL_WIDTH, initial_height - SCROLL_TOP - FOOTER_HEIGHT),
                )
            ]
        };
        scroll.setBorderType(objc2_app_kit::NSBorderType::NoBorder);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setVerticalScrollElasticity(NSScrollElasticity::None);
        scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        scroll.setHasVerticalScroller(true);
        scroll.setHasHorizontalScroller(false);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.contentView().setDrawsBackground(false);
        scroll.setAutoresizingMask(
            objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        scroll.setDocumentView(Some(&document));
        document.setAutoresizesSubviews(false);
        root.addSubview(&scroll);

        let character_tab = MenuPanelDocument::new(
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(PANEL_WIDTH, PANEL_HEIGHT),
            ),
            mtm,
        );
        character_tab.setAutoresizesSubviews(false);
        set_accessibility_element(&*character_tab, false);
        let bubble_tab = MenuPanelDocument::new(
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(PANEL_WIDTH, PANEL_HEIGHT),
            ),
            mtm,
        );
        bubble_tab.setAutoresizesSubviews(false);
        set_accessibility_element(&*bubble_tab, false);
        let settings_tab = MenuPanelDocument::new(
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(PANEL_WIDTH, PANEL_HEIGHT),
            ),
            mtm,
        );
        settings_tab.setAutoresizesSubviews(false);
        set_accessibility_element(&*settings_tab, false);
        document.addSubview(&character_tab);
        document.addSubview(&bubble_tab);
        document.addSubview(&settings_tab);
        bubble_tab.setHidden(true);
        settings_tab.setHidden(true);
        let available = NSScreen::mainScreen(mtm)
            .map(|screen| screen.visibleFrame().size)
            .unwrap_or(NSSize::new(800.0, 700.0));
        let detail_size = NSSize::new(
            DETAIL_DEFAULT_WIDTH.min((available.width - 32.0).max(1.0)),
            DETAIL_DEFAULT_MAX_HEIGHT.min((available.height - 64.0).max(1.0)),
        );
        let detail_frame = NSRect::new(NSPoint::new(0.0, 0.0), detail_size);
        let detail_window = SettingsDetailNativeWindow::new(detail_frame, mtm);
        unsafe { detail_window.setReleasedWhenClosed(false) };
        detail_window.setContentMinSize(NSSize::new(
            DETAIL_MIN_WIDTH.min(detail_size.width),
            DETAIL_MIN_HEIGHT.min(detail_size.height),
        ));
        detail_window.center();
        if let Some(appearance) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua })
        {
            let _: () = unsafe { msg_send![&*detail_window, setAppearance: Some(&*appearance)] };
        }
        let detail_root = SettingsDetailRoot::new(detail_frame, mtm);
        detail_root.setAutoresizingMask(
            objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        detail_window.setContentView(Some(&detail_root));
        let detail_scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), detail_frame);
        detail_scroll.setBorderType(objc2_app_kit::NSBorderType::NoBorder);
        detail_scroll.setHasVerticalScroller(true);
        detail_scroll.setHasHorizontalScroller(false);
        detail_scroll.setAutohidesScrollers(true);
        detail_scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        detail_scroll.setDrawsBackground(false);
        detail_scroll.contentView().setDrawsBackground(false);
        detail_scroll.setVerticalScrollElasticity(NSScrollElasticity::None);
        detail_scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        let detail_document = MenuPanelDocument::new(detail_frame, mtm);
        detail_document.setAutoresizesSubviews(false);
        detail_scroll.setDocumentView(Some(&detail_document));
        detail_root.addSubview(&detail_scroll);
        let detail_documents = std::array::from_fn(|_| {
            let document = MenuPanelDocument::new(detail_frame, mtm);
            document.setAutoresizesSubviews(false);
            document.setHidden(true);
            set_accessibility_element(&document, false);
            detail_document.addSubview(&document);
            document
        });
        let detail_openers = std::array::from_fn(|index| {
            let button = make_action_button("", target, sel!(openSettingsDetail:), mtm);
            button.setTag(index as isize);
            button
        });
        let appearance_error = diagnostic_label(mtm);
        let lifecycle_error_detail = diagnostic_label(mtm);
        let connection_summary = label("", 12.0, true, primary(), mtm);
        let connection_selection_summary = label("", 12.0, true, primary(), mtm);
        let connection_error_summary = label("", 11.0, false, NSColor::systemRedColor(), mtm);
        for field in [
            &connection_summary,
            &connection_selection_summary,
            &connection_error_summary,
        ] {
            field.setMaximumNumberOfLines(0);
        }
        let connection_settings_error = diagnostic_label(mtm);
        let footer_details = make_action_button("", target, sel!(openSettingsDetail:), mtm);
        footer_details.setTag(SettingsDetailPage::Appearance as isize);
        footer_details.setHidden(true);
        let app_general_card = MenuPanelCard::new(NSRect::default(), mtm);

        let title = label(
            desktop_pet_update_coordinator::identity::DISPLAY_NAME,
            14.0,
            true,
            primary(),
            mtm,
        );
        let panel_title = label(
            text(locale, Message::MenuPanelTitle),
            11.0,
            false,
            secondary(),
            mtm,
        );
        panel.setTitle(&NSString::from_str(text(locale, Message::MenuPanelTitle)));
        root.addSubview(&title);
        root.addSubview(&panel_title);

        let tab_track = MenuTabTrack::new(NSRect::default(), mtm);
        set_accessibility_element(&*tab_track, false);
        root.addSubview(&tab_track);

        let tab_indicator = make_tab_indicator(mtm);
        root.addSubview(&tab_indicator);

        let tabs = [
            make_tab(
                "person.crop.circle.fill",
                text(locale, Message::MenuAppearanceTab),
                0,
                target,
                mtm,
            ),
            make_tab(
                "bubble.left.and.bubble.right.fill",
                text(locale, Message::MenuConnectionsTab),
                1,
                target,
                mtm,
            ),
            make_tab(
                "gearshape.fill",
                text(locale, Message::MenuAppTab),
                2,
                target,
                mtm,
            ),
        ];
        for tab in &tabs {
            root.addSubview(tab);
        }

        let character_card = MenuPanelCard::new(NSRect::default(), mtm);
        set_accessibility_element(&*character_card, false);
        let character_visible_label = label(
            text(locale, Message::MenuCharacterVisible),
            12.5,
            true,
            primary(),
            mtm,
        );
        let character_visible_switch = make_switch(target, sel!(hide:), mtm);
        set_accessibility_label(
            &*character_visible_switch,
            text(locale, Message::MenuCharacterVisible),
        );
        let character_browser_open = make_action_button(
            text(locale, Message::CharacterBrowserOpen),
            target,
            sel!(openCharacterBrowser:),
            mtm,
        );
        set_accessibility_identifier(&character_browser_open, "herdr.character.open-browser");
        set_accessibility_label(
            &character_browser_open,
            text(locale, Message::CharacterBrowserOpen),
        );
        character_browser_open.setFont(Some(&NSFont::systemFontOfSize(11.5)));
        character_browser_open.setImageHugsTitle(true);
        character_browser_open.setImagePosition(NSCellImagePosition::ImageLeading);
        let browser_symbol = NSString::from_str("square.grid.2x2");
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &browser_symbol,
            Some(&NSString::from_str(text(
                locale,
                Message::CharacterBrowserOpen,
            ))),
        ) {
            character_browser_open.setImage(Some(&image));
        }
        set_tooltip(
            &character_browser_open,
            text(locale, Message::CharacterBrowserOpen),
        );
        let character_editing_open = make_action_button(
            text(locale, Message::CharacterEditingOpen),
            target,
            sel!(openDialogueEditor:),
            mtm,
        );
        character_editing_open.setFont(Some(&NSFont::systemFontOfSize(11.5)));
        character_editing_open.setImageHugsTitle(true);
        character_editing_open.setImagePosition(NSCellImagePosition::ImageLeading);
        let symbol_name = NSString::from_str("bubble.left.and.text.bubble.right");
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &symbol_name,
            Some(&NSString::from_str(text(
                locale,
                Message::CharacterEditingOpen,
            ))),
        ) {
            character_editing_open.setImage(Some(&image));
        }
        set_tooltip(
            &character_editing_open,
            text(locale, Message::CharacterEditingOpen),
        );
        let character_view = character_view.retain();
        character_card.addSubview(&character_visible_label);
        character_card.addSubview(&*character_visible_switch);
        character_tab.addSubview(&character_card);
        character_tab.addSubview(&character_browser_open);
        detail_documents[0].addSubview(&character_editing_open);
        character_tab.addSubview(&character_view);

        let bubble_visibility_card = MenuPanelCard::new(NSRect::default(), mtm);
        set_accessibility_element(&*bubble_visibility_card, false);
        let bubble_visible_label = label(
            text(locale, Message::MenuBubbleVisible),
            12.5,
            true,
            primary(),
            mtm,
        );
        let bubble_visible_switch = make_switch(target, sel!(hideBubble:), mtm);
        set_accessibility_label(
            &*bubble_visible_switch,
            text(locale, Message::MenuBubbleVisible),
        );
        bubble_visibility_card.addSubview(&bubble_visible_label);
        bubble_visibility_card.addSubview(&*bubble_visible_switch);
        character_tab.addSubview(&bubble_visibility_card);

        let status_indicators_label = label(
            text(locale, Message::MenuStatusIndicators),
            12.5,
            true,
            primary(),
            mtm,
        );
        let status_indicators_switch = make_switch(target, sel!(toggleStatusIndicators:), mtm);
        status_indicators_switch.setState(NSControlStateValueOn);
        set_accessibility_label(
            &*status_indicators_switch,
            text(locale, Message::MenuStatusIndicators),
        );
        detail_documents[0].addSubview(&status_indicators_label);
        detail_documents[0].addSubview(&*status_indicators_switch);

        let bubble_placement_label = label(
            text(locale, Message::MenuBubblePlacement),
            12.5,
            true,
            primary(),
            mtm,
        );
        bubble_visibility_card.addSubview(&bubble_placement_label);
        let placement_specs = [
            (Message::MenuPlacementAuto, sel!(bubbleAuto:)),
            (Message::MenuPlacementAbove, sel!(bubbleAbove:)),
            (Message::MenuPlacementBelow, sel!(bubbleBelow:)),
            (Message::MenuPlacementLeft, sel!(bubbleLeft:)),
            (Message::MenuPlacementRight, sel!(bubbleRight:)),
        ];
        let placement_buttons = [
            make_segment_button(
                text(locale, placement_specs[0].0),
                target,
                placement_specs[0].1,
                mtm,
            ),
            make_segment_button(
                text(locale, placement_specs[1].0),
                target,
                placement_specs[1].1,
                mtm,
            ),
            make_segment_button(
                text(locale, placement_specs[2].0),
                target,
                placement_specs[2].1,
                mtm,
            ),
            make_segment_button(
                text(locale, placement_specs[3].0),
                target,
                placement_specs[3].1,
                mtm,
            ),
            make_segment_button(
                text(locale, placement_specs[4].0),
                target,
                placement_specs[4].1,
                mtm,
            ),
        ];
        for button in &placement_buttons {
            bubble_visibility_card.addSubview(button);
        }
        let bubble_theme_card = MenuPanelCard::new(NSRect::default(), mtm);
        set_accessibility_element(&*bubble_theme_card, false);
        let bubble_theme_label = label(
            text(locale, Message::BubbleTheme),
            12.5,
            true,
            primary(),
            mtm,
        );
        let theme_specs = [
            (Message::ThemeWarmIvory, 0),
            (Message::ThemeDustyRose, 1),
            (Message::ThemeMoonlitInk, 2),
            (Message::ThemeCustom, 3),
        ];
        let bubble_theme_popup =
            make_selection_popup(&theme_specs, locale, target, sel!(setBubbleTheme:), mtm);
        bubble_visibility_card.addSubview(&bubble_theme_label);
        bubble_visibility_card.addSubview(&bubble_theme_popup);
        let bubble_colors_label = label(
            text(locale, Message::CustomizeBubbleColors),
            11.0,
            false,
            secondary(),
            mtm,
        );
        bubble_colors_label.setMaximumNumberOfLines(0);
        let color_messages = [
            Message::BubbleSurfaceColor,
            Message::BubbleTextColor,
            Message::BubbleMutedColor,
            Message::BubbleBorderColor,
            Message::BubbleAccentColor,
        ];
        let bubble_color_labels =
            color_messages.map(|message| label(text(locale, message), 11.0, false, primary(), mtm));
        let bubble_color_fields = color_messages.map(|message| {
            let field = SettingsColorNativeField::new(mtm);
            field.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(12.0, 0.0)));
            field.setTextColor(Some(&primary()));
            field.setEditable(true);
            field.setSelectable(true);
            field.setBezeled(true);
            field.setRefusesFirstResponder(false);
            set_accessibility_label(&field, text(locale, message));
            field
        });
        let bubble_color_swatches = [
            make_color_swatch(mtm),
            make_color_swatch(mtm),
            make_color_swatch(mtm),
            make_color_swatch(mtm),
            make_color_swatch(mtm),
        ];
        let bubble_apply = make_action_button(
            text(locale, Message::ApplyBubbleColors),
            target,
            sel!(applyBubbleColors:),
            mtm,
        );
        let bubble_reset = make_action_button(
            text(locale, Message::ResetBubbleColors),
            target,
            sel!(resetBubbleColors:),
            mtm,
        );
        let bubble_reload = make_action_button(
            text(locale, Message::BubbleColorsReload),
            target,
            sel!(reloadBubbleColors:),
            mtm,
        );
        let bubble_rebase = make_action_button(
            text(locale, Message::BubbleColorsRebase),
            target,
            sel!(rebaseBubbleColors:),
            mtm,
        );
        set_accessibility_label(&bubble_apply, text(locale, Message::ApplyBubbleColors));
        set_accessibility_label(&bubble_reset, text(locale, Message::ResetBubbleColors));
        bubble_theme_card.addSubview(&bubble_colors_label);
        for ((name, field), swatch) in bubble_color_labels
            .iter()
            .zip(&bubble_color_fields)
            .zip(&bubble_color_swatches)
        {
            bubble_theme_card.addSubview(name);
            bubble_theme_card.addSubview(swatch);
            bubble_theme_card.addSubview(field);
        }
        bubble_theme_card.addSubview(&bubble_apply);
        bubble_theme_card.addSubview(&bubble_reset);
        bubble_theme_card.addSubview(&bubble_reload);
        bubble_theme_card.addSubview(&bubble_rebase);
        detail_documents[0].addSubview(&bubble_theme_card);

        let appearance_card = MenuPanelCard::new(NSRect::default(), mtm);
        set_accessibility_element(&*appearance_card, false);
        let settings_appearance_label = label(
            text(locale, Message::MenuAppearance),
            12.5,
            true,
            primary(),
            mtm,
        );
        let click_behavior_label = label(
            text(locale, Message::MenuClickBehavior),
            11.0,
            false,
            secondary(),
            mtm,
        );
        let full_passthrough_label = label(
            text(locale, Message::MenuFullPassthrough),
            12.0,
            true,
            primary(),
            mtm,
        );
        let full_passthrough_help = label(
            text(locale, Message::MenuFullPassthroughHelp),
            10.0,
            false,
            secondary(),
            mtm,
        );
        let full_passthrough_switch = make_switch(target, sel!(togglePassthrough:), mtm);
        let alpha_passthrough_label = label(
            text(locale, Message::MenuAlphaPassthrough),
            12.0,
            true,
            primary(),
            mtm,
        );
        let alpha_passthrough_help = label(
            text(locale, Message::MenuAlphaPassthroughHelp),
            10.0,
            false,
            secondary(),
            mtm,
        );
        let alpha_passthrough_switch = make_switch(target, sel!(toggleAlphaPassthrough:), mtm);
        for view in [
            &*settings_appearance_label as &NSView,
            &*click_behavior_label,
            &*full_passthrough_label,
            &*full_passthrough_help,
            &*full_passthrough_switch,
            &*alpha_passthrough_label,
            &*alpha_passthrough_help,
            &*alpha_passthrough_switch,
        ] {
            appearance_card.addSubview(view);
        }
        set_accessibility_label(
            &*full_passthrough_switch,
            text(locale, Message::MenuFullPassthrough),
        );
        set_accessibility_label(
            &*alpha_passthrough_switch,
            text(locale, Message::MenuAlphaPassthrough),
        );

        let scale_card = MenuPanelCard::new(NSRect::default(), mtm);
        set_accessibility_element(&*scale_card, false);
        let scale_label = label(text(locale, Message::MenuScale), 12.5, true, primary(), mtm);
        let scale_down = make_stepper_button("−", target, sel!(scaleDown:), mtm);
        let scale_up = make_stepper_button("+", target, sel!(scaleUp:), mtm);
        let scale_readout = label("0.65×", 12.0, true, primary(), mtm);
        scale_readout.setAlignment(NSTextAlignment::Center);
        set_accessibility_label(&*scale_down, text(locale, Message::ScaleDown));
        set_accessibility_label(&*scale_up, text(locale, Message::ScaleUp));
        set_accessibility_label(&*scale_readout, text(locale, Message::MenuScale));
        scale_card.addSubview(&scale_label);
        scale_card.addSubview(&scale_down);
        scale_card.addSubview(&scale_up);
        scale_card.addSubview(&scale_readout);

        let language_label = label(
            text(locale, Message::MenuLanguage),
            12.5,
            true,
            primary(),
            mtm,
        );
        let language_specs = [
            (Message::LanguageSystem, 0),
            (Message::KoreanLanguage, 1),
            (Message::EnglishLanguage, 2),
            (Message::SimplifiedChineseLanguage, 3),
            (Message::JapaneseLanguage, 4),
        ];
        let language_popup =
            make_selection_popup(&language_specs, locale, target, sel!(setLanguage:), mtm);
        let menu_bar_card = MenuPanelCard::new(NSRect::default(), mtm);
        set_accessibility_element(&*menu_bar_card, false);
        app_general_card.addSubview(&language_label);
        app_general_card.addSubview(&language_popup);
        let menu_bar_label = label(
            text(locale, Message::MenuGeneral),
            12.5,
            true,
            primary(),
            mtm,
        );
        let menu_bar_preview_well = NSBox::initWithFrame(NSBox::alloc(mtm), NSRect::default());
        menu_bar_preview_well.setBoxType(NSBoxType::Custom);
        menu_bar_preview_well.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        menu_bar_preview_well.setContentViewMargins(NSSize::new(0.0, 0.0));
        menu_bar_preview_well.setTransparent(false);
        menu_bar_preview_well.setBorderWidth(1.0);
        menu_bar_preview_well.setCornerRadius(6.0);
        menu_bar_preview_well.setFillColor(&NSColor::controlBackgroundColor());
        menu_bar_preview_well.setBorderColor(&color(
            CARD_BORDER_RED,
            CARD_BORDER_GREEN,
            CARD_BORDER_BLUE,
            CARD_BORDER_ALPHA,
        ));
        set_accessibility_element(&*menu_bar_preview_well, false);
        let menu_bar_preview =
            NSImageView::initWithFrame(NSImageView::alloc(mtm), NSRect::default());
        menu_bar_preview.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        menu_bar_preview.setEditable(false);
        set_accessibility_element(&*menu_bar_preview, false);
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("pawprint.fill"),
            None,
        ) {
            image.setTemplate(true);
            menu_bar_preview.setImage(Some(&image));
        }
        menu_bar_preview_well.addSubview(&menu_bar_preview);
        let menu_bar_icon_status = label(
            text(locale, Message::MenuBarIconDefault),
            11.5,
            true,
            primary(),
            mtm,
        );
        set_accessibility_identifier(&menu_bar_icon_status, "menu-bar-icon-status");
        let menu_bar_visibility = label(
            text(locale, Message::MenuBarVisibility),
            11.0,
            true,
            secondary(),
            mtm,
        );
        set_accessibility_element(&*menu_bar_visibility, false);
        let menu_bar_popup = make_selection_popup(
            &[
                (Message::MenuBarAlways, 0),
                (Message::MenuBarRecoveryOnly, 1),
            ],
            locale,
            target,
            sel!(setMenuBarMode:),
            mtm,
        );
        set_accessibility_identifier(&menu_bar_popup, "menu-bar-visibility");
        let menu_bar_help = label(
            text(locale, Message::MenuBarModeHelp),
            10.0,
            false,
            secondary(),
            mtm,
        );
        menu_bar_help.setMaximumNumberOfLines(0);
        let menu_bar_separator = NSBox::initWithFrame(NSBox::alloc(mtm), NSRect::default());
        menu_bar_separator.setBoxType(NSBoxType::Custom);
        menu_bar_separator.setBorderWidth(0.0);
        menu_bar_separator.setFillColor(&color(
            CARD_BORDER_RED,
            CARD_BORDER_GREEN,
            CARD_BORDER_BLUE,
            0.35,
        ));
        set_accessibility_element(&*menu_bar_separator, false);
        let menu_bar_choose = make_action_button(
            text(locale, Message::MenuBarIconChoose),
            target,
            sel!(chooseMenuBarIcon:),
            mtm,
        );
        set_accessibility_identifier(&menu_bar_choose, "menu-bar-icon-choose");
        let menu_bar_restore = make_action_button(
            text(locale, Message::MenuBarIconRestore),
            target,
            sel!(resetMenuBarIcon:),
            mtm,
        );
        set_accessibility_identifier(&menu_bar_restore, "menu-bar-icon-reset");
        menu_bar_restore.setEnabled(false);
        let menu_bar_icon_help = label(
            text(locale, Message::MenuBarIconHelp),
            10.0,
            false,
            secondary(),
            mtm,
        );
        menu_bar_icon_help.setMaximumNumberOfLines(0);
        let menu_bar_icon_error = label("", 10.5, false, NSColor::systemRedColor(), mtm);
        menu_bar_icon_error.setMaximumNumberOfLines(0);
        menu_bar_icon_error.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByCharWrapping);
        set_accessibility_identifier(&menu_bar_icon_error, "menu-bar-icon-error");
        menu_bar_icon_error.setHidden(true);
        menu_bar_card.addSubview(&menu_bar_label);
        menu_bar_card.addSubview(&menu_bar_visibility);
        menu_bar_card.addSubview(&menu_bar_popup);
        menu_bar_card.addSubview(&menu_bar_help);
        menu_bar_card.addSubview(&menu_bar_separator);
        menu_bar_card.addSubview(&menu_bar_preview_well);
        menu_bar_card.addSubview(&menu_bar_icon_status);
        menu_bar_card.addSubview(&menu_bar_choose);
        menu_bar_card.addSubview(&menu_bar_restore);
        menu_bar_card.addSubview(&menu_bar_icon_help);
        menu_bar_card.addSubview(&menu_bar_icon_error);
        let observation_card = MenuPanelCard::new(NSRect::default(), mtm);
        set_accessibility_element(&*observation_card, false);
        let observation_title = label(
            text(locale, Message::ConnectionTitle),
            12.5,
            true,
            primary(),
            mtm,
        );
        let connection_host_label = label(
            text(locale, Message::ConnectionHost),
            11.5,
            true,
            primary(),
            mtm,
        );
        let connection_host_popup = make_selection_popup(
            &[(Message::ConnectionHostAuto, 0)],
            locale,
            target,
            sel!(setObservationHost:),
            mtm,
        );
        let connection_status = label("", 10.5, false, secondary(), mtm);
        connection_status.setMaximumNumberOfLines(0);
        let connection_auto_help = label(
            text(locale, Message::ConnectionAutoHelp),
            10.0,
            false,
            secondary(),
            mtm,
        );
        connection_auto_help.setMaximumNumberOfLines(0);
        let connection_profile_title = label(
            text(locale, Message::ConnectionProfile),
            11.0,
            true,
            primary(),
            mtm,
        );
        let connection_profile_labels =
            std::array::from_fn(|_| label("", 10.0, true, secondary(), mtm));
        let connection_profile_values = std::array::from_fn(|_| {
            let field = label("", 10.5, false, primary(), mtm);
            field.setMaximumNumberOfLines(0);
            field.setSelectable(true);
            field.setRefusesFirstResponder(false);
            field
        });
        for view in [
            &*connection_host_label as &NSView,
            &*connection_host_popup,
            &*connection_status,
            &*connection_auto_help,
            &*connection_settings_error,
        ] {
            detail_documents[1].addSubview(view);
        }
        detail_documents[4].addSubview(&connection_profile_title);
        for field in connection_profile_labels
            .iter()
            .chain(connection_profile_values.iter())
        {
            detail_documents[4].addSubview(field);
        }
        let observation_error = label("", 10.0, false, secondary(), mtm);
        observation_error.setMaximumNumberOfLines(0);
        observation_error.setSelectable(true);
        observation_error.setHidden(true);
        observation_card.addSubview(&observation_title);
        detail_documents[4].addSubview(&observation_error);
        detail_documents[4].addSubview(&lifecycle_error_detail);
        detail_documents[0].addSubview(&appearance_error);
        detail_documents[0].addSubview(&appearance_card);
        detail_documents[2].addSubview(&menu_bar_card);
        character_tab.addSubview(&scale_card);
        settings_tab.addSubview(&app_general_card);
        bubble_tab.addSubview(&observation_card);
        for view in [
            &*connection_selection_summary as &NSView,
            &*connection_summary,
            &*connection_error_summary,
        ] {
            observation_card.addSubview(view);
        }
        character_tab.addSubview(&detail_openers[0]);
        for index in [1, 4] {
            bubble_tab.addSubview(&detail_openers[index]);
        }
        for index in [2, 3, 5, 6] {
            settings_tab.addSubview(&detail_openers[index]);
        }
        let mut lifecycle_card = LifecycleSettingsCard::new(locale, target, mtm);
        detail_documents[1].addSubview(lifecycle_card.separate_extensions());
        settings_tab.addSubview(lifecycle_card.view());
        let update_card = AppUpdateCard::new(locale, target, mtm);
        settings_tab.addSubview(update_card.view());
        detail_documents[3].addSubview(update_card.settings_view());
        detail_documents[5].addSubview(update_card.details_view());
        let recovery_reset_help = diagnostic_label(mtm);
        let recovery_preserved_help = diagnostic_label(mtm);
        let reset_settings = make_action_button(
            text(locale, Message::ResetAppSettings),
            target,
            sel!(resetAppSettings:),
            mtm,
        );
        reset_settings.setKeyEquivalent(&NSString::from_str(""));
        set_accessibility_label(&reset_settings, text(locale, Message::ResetAppSettings));
        for (field, message) in [
            (&recovery_reset_help, Message::AppRecoveryResetHelp),
            (&recovery_preserved_help, Message::AppRecoveryPreservedHelp),
        ] {
            set_readonly_value(field, text(locale, message));
            set_accessibility_label(field, text(locale, message));
            detail_documents[SettingsDetailPage::AppRecovery.index()].addSubview(field);
        }
        detail_documents[SettingsDetailPage::AppRecovery.index()].addSubview(&reset_settings);
        let settings_recovery_status = diagnostic_label(mtm);
        settings_recovery_status.setMaximumNumberOfLines(8);
        settings_recovery_status.setHidden(true);
        detail_documents[SettingsDetailPage::AppRecovery.index()]
            .addSubview(&settings_recovery_status);

        let status = label("", 10.5, false, secondary(), mtm);
        status.setMaximumNumberOfLines(2);
        let reset = make_action_button(
            text(locale, Message::ResetPosition),
            target,
            sel!(resetPosition:),
            mtm,
        );
        let restart = make_action_button(
            text(locale, Message::MenuRestartApp),
            target,
            sel!(restartApp:),
            mtm,
        );
        let quit = make_action_button(text(locale, Message::MenuQuit), target, sel!(quit:), mtm);
        reset.setKeyEquivalent(&NSString::from_str(""));
        restart.setKeyEquivalent(&NSString::from_str(""));
        quit.setKeyEquivalent(&NSString::from_str(""));
        set_accessibility_label(&*reset, text(locale, Message::ResetPosition));
        set_accessibility_label(&*restart, text(locale, Message::MenuRestartApp));
        set_accessibility_label(&*quit, text(locale, Message::MenuQuit));
        root.addSubview(&status);
        character_tab.addSubview(&reset);
        root.addSubview(&footer_details);
        root.addSubview(&restart);
        root.addSubview(&quit);

        let mut panel = Self {
            panel,
            root,
            tab_track,
            tab_indicator,
            scroll,
            document,
            character_tab,
            bubble_tab,
            settings_tab,
            detail_window,
            detail_root,
            detail_scroll,
            detail_document,
            detail_documents,
            selected_detail: None,
            detail_offsets: Cell::new([0.0; DETAIL_PAGE_COUNT]),
            detail_user_sizes: Cell::new([None; DETAIL_PAGE_COUNT]),
            detail_layout_size: Cell::new(None),
            detail_openers,
            saved_color_selection: None,
            character_card,
            bubble_visibility_card,
            bubble_theme_card,
            detail_opener: None,
            appearance_card,
            scale_card,
            app_general_card,
            appearance_error,
            lifecycle_error_detail,
            connection_summary,
            connection_selection_summary,
            connection_error_summary,
            connection_settings_error,
            connection_query_error: None,
            observation_locale: None,
            connection_operation_error: None,
            connection_snapshot_unavailable: false,
            footer_details,
            update_frozen: false,
            reset_settings_busy: false,
            reset_settings,
            recovery_reset_help,
            recovery_preserved_help,
            settings_recovery_status,
            control_refresh_depth: 0,
            update_details_problem: false,
            tabs,
            placement_buttons,
            character_view,
            character_content_height: INITIAL_CHARACTER_HEIGHT,
            character_editing_open,
            character_browser_open,
            target: target.retain(),
            title,
            panel_title,
            character_visible_label,
            character_visible_switch,
            bubble_visible_label,
            bubble_visible_switch,
            status_indicators_label,
            status_indicators_switch,
            bubble_placement_label,
            bubble_theme_label,
            bubble_theme_popup,
            bubble_colors_label,
            bubble_color_labels,
            bubble_color_fields,
            bubble_color_swatches,
            bubble_apply,
            bubble_reset,
            bubble_reload,
            bubble_rebase,
            bubble_draft: BubbleColorDraft::default(),
            bubble_control_state: None,
            bubble_conflict_label: None,
            settings_appearance_label,
            click_behavior_label,
            full_passthrough_label,
            full_passthrough_help,
            full_passthrough_switch,
            alpha_passthrough_label,
            alpha_passthrough_help,
            alpha_passthrough_switch,
            scale_label,
            scale_readout,
            scale_down,
            scale_up,
            language_label,
            language_popup,
            menu_bar_card,
            menu_bar_label,
            menu_bar_preview_well,
            menu_bar_preview,
            menu_bar_separator,
            menu_bar_icon_status,
            menu_bar_visibility,
            menu_bar_popup,
            menu_bar_help,
            menu_bar_choose,
            menu_bar_restore,
            menu_bar_icon_help,
            menu_bar_icon_error,
            menu_bar_icon_error_detail: None,
            menu_bar_icon_custom: false,
            menu_bar_icon_busy: Cell::new(false),
            lifecycle_card,
            frozen_controls: Vec::new(),
            update_card,
            observation_card,
            observation_title,
            connection_host_label,
            connection_host_popup,
            connection_status,
            connection_auto_help,
            connection_profile_title,
            connection_profile_labels,
            connection_profile_values,
            connection_hosts: HostConnections::default(),
            provider_labels: Default::default(),
            provider_ids: Vec::new(),
            observation_error,
            observation_preferences: None,
            presentation_error: None,
            status_text: String::new(),
            locale,
            status,
            reset,
            restart,
            quit,
            selected_tab: SettingsTab::Appearance,
            tab_offsets: Cell::new([0.0; 3]),
        };
        panel.set_menu_bar_mode(MenuBarMode::Always);
        panel.select_tab(SettingsTab::Appearance);
        panel.update_color_swatches();
        panel.set_locale(locale);
        panel.layout_root();
        panel
    }

    pub(crate) fn show(&self) {
        if !self.panel.isVisible() {
            self.panel.makeKeyAndOrderFront(None);
            let first = &self.tabs[self.selected_tab.index()];
            self.panel.makeFirstResponder(Some(&**first));
        } else {
            self.panel.makeKeyAndOrderFront(None);
        }
    }

    pub(crate) fn hide(&self) {
        if self.panel.isKeyWindow() {
            self.panel.resignKeyWindow();
        }
        self.panel.orderOut(None);
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.panel.isVisible()
    }

    pub(crate) fn set_character_content_height(&mut self, height: f64) -> bool {
        if self.character_content_height == height {
            return false;
        }
        self.character_content_height = height;
        true
    }

    pub(crate) fn character_frame(&self) -> NSRect {
        self.character_view.frame()
    }

    pub(crate) fn select_tab(&mut self, tab: SettingsTab) {
        let index = tab.index();
        let mut offsets = self.tab_offsets.get();
        offsets[self.selected_tab.index()] = self.scroll.contentView().bounds().origin.y;
        self.tab_offsets.set(offsets);
        self.character_tab.setHidden(tab != SettingsTab::Appearance);
        self.bubble_tab.setHidden(tab != SettingsTab::Connections);
        self.settings_tab.setHidden(tab != SettingsTab::App);
        self.selected_tab = tab;
        self.apply_tab_selection();
        self.layout_documents();
        self.panel.recalculateKeyViewLoop();
        let maximum = (self.document.frame().size.height
            - self.scroll.contentView().bounds().size.height)
            .max(0.0);
        self.scroll
            .contentView()
            .scrollToPoint(NSPoint::new(0.0, offsets[index].clamp(0.0, maximum)));
        self.scroll
            .reflectScrolledClipView(&self.scroll.contentView());
        if self.panel.isKeyWindow() {
            self.panel.makeFirstResponder(Some(&*self.tabs[index]));
        }
    }

    pub(crate) fn prepare_detail(
        &mut self,
        page: SettingsDetailPage,
    ) -> Option<Retained<NSWindow>> {
        if self.bubble_colors_marked() || self.menu_bar_icon_busy.get() {
            return None;
        }
        let changed = self.selected_detail != Some(page);
        let reopening = !self.detail_window.isVisible();
        self.remember_detail_size();
        if changed {
            self.remember_color_selection();
            if !self.detail_window.makeFirstResponder(None) {
                return None;
            }
        }
        if !reopening {
            if let Some(old) = self.selected_detail {
                let mut offsets = self.detail_offsets.get();
                offsets[old.index()] = self.detail_scroll.contentView().bounds().origin.y;
                self.detail_offsets.set(offsets);
            }
        }
        self.selected_detail = Some(page);
        for (index, document) in self.detail_documents.iter().enumerate() {
            document.setHidden(index != page.index());
        }
        self.detail_window
            .setTitle(&NSString::from_str(text(self.locale, page.message())));
        self.layout_detail();
        if changed || reopening {
            self.restore_detail_offset(page);
            if page == SettingsDetailPage::Appearance {
                self.restore_color_selection();
            }
        }
        Some(self.detail_window.retain().into_super())
    }
    pub(crate) fn set_detail_opener(&mut self, opener: Option<&NSButton>) {
        self.detail_opener = opener.map(|button| button.retain());
    }

    pub(crate) fn detail_page(&self) -> Option<SettingsDetailPage> {
        self.detail_window
            .isVisible()
            .then_some(self.selected_detail)
            .flatten()
    }
    pub(crate) fn detail_window(&self) -> &NSWindow {
        &self.detail_window
    }
    pub(crate) fn settings_surface_visible(&self) -> bool {
        self.is_visible() || self.detail_window.isVisible()
    }
    pub(crate) fn appearance_detail_visible(&self) -> bool {
        self.detail_page() == Some(SettingsDetailPage::Appearance)
    }
    pub(crate) fn connection_content_visible(&self) -> bool {
        (self.is_visible() && self.selected_tab == SettingsTab::Connections)
            || matches!(
                self.detail_page(),
                Some(SettingsDetailPage::Connections | SettingsDetailPage::ConnectionDiagnostics)
            )
    }
    pub(crate) fn refresh_detail_layout(&mut self) {
        if self.detail_root.ivars().layout_dirty.get() && !self.bubble_colors_marked() {
            self.layout_detail();
        }
    }
    fn remember_color_selection(&mut self) {
        for index in 0..self.bubble_color_fields.len() {
            if let Some(editor) = self.bubble_field_editor(index) {
                let selection = unsafe { msg_send![editor, selectedRange] };
                self.saved_color_selection = Some((index, selection));
                break;
            }
        }
    }
    fn restore_color_selection(&self) {
        let Some((index, range)) = self.saved_color_selection else {
            return;
        };
        if self.update_frozen {
            return;
        }
        let length = self.bubble_color_fields[index].stringValue().length();
        if range.location <= length && range.length <= length - range.location {
            self.detail_window
                .makeFirstResponder(Some(&*self.bubble_color_fields[index]));
            if let Some(editor) = self.bubble_field_editor(index) {
                let _: () = unsafe { msg_send![editor, setSelectedRange: range] };
            }
        }
    }
    pub(crate) fn finish_detail_close(&mut self) -> bool {
        if self.bubble_colors_marked() || self.menu_bar_icon_busy.get() {
            return false;
        }
        self.remember_color_selection();
        if !self.detail_window.makeFirstResponder(None) {
            return false;
        }
        self.remember_detail_size();
        if let Some(page) = self.selected_detail {
            let mut offsets = self.detail_offsets.get();
            offsets[page.index()] = self.detail_scroll.contentView().bounds().origin.y;
            self.detail_offsets.set(offsets);
        }
        let focus_opener = self.detail_window.isKeyWindow()
            && self.panel.isVisible()
            && objc2_app_kit::NSApplication::sharedApplication(self.panel.mtm()).isActive();
        self.detail_window.orderOut(None);
        if focus_opener {
            self.panel.makeKeyAndOrderFront(None);
            if let Some(page) = self.selected_detail {
                let opener = self
                    .detail_opener
                    .as_ref()
                    .unwrap_or(&self.detail_openers[page.index()]);
                let hidden: bool = unsafe { msg_send![&**opener, isHiddenOrHasHiddenAncestor] };
                if !hidden {
                    self.panel.makeFirstResponder(Some(&**opener));
                }
            }
        }
        true
    }

    fn bubble_field_editor(&self, index: usize) -> Option<&AnyObject> {
        let field = &self.bubble_color_fields[index];
        let window = field.window()?;
        let editor: Option<&AnyObject> = unsafe { msg_send![&**field, currentEditor] };
        let editor = editor?;
        let responder: Option<&AnyObject> = unsafe { msg_send![&*window, firstResponder] };
        if !responder.is_some_and(|responder| std::ptr::eq(responder, editor)) {
            return None;
        }
        if let Some(text) = editor.downcast_ref::<NSTextView>() {
            if !text.allowsUndo() {
                text.setAllowsUndo(true);
            }
        }
        Some(editor)
    }

    pub(crate) fn bubble_colors_marked(&self) -> bool {
        (0..self.bubble_color_fields.len()).any(|index| {
            self.bubble_field_editor(index)
                .is_some_and(|editor| unsafe { msg_send![editor, hasMarkedText] })
        })
    }

    fn bubble_field_dirty(&self, index: usize) -> bool {
        let Some(baseline) = self.bubble_draft.baseline else {
            return false;
        };
        let palette = baseline.palette();
        let saved = [
            palette.surface,
            palette.text,
            palette.muted,
            palette.border,
            palette.accent,
        ][index];
        self.bubble_color_fields[index].stringValue().to_string() != saved.to_hex()
    }

    pub(crate) fn refresh_bubble_color_controls(&mut self) {
        self.begin_control_refresh();
        let conflicted = self.bubble_draft.conflicted();
        let marked = self.bubble_colors_marked();
        let reading_position =
            (self.bubble_conflict_label != Some(conflicted)).then(|| self.reading_position());
        if self.bubble_control_state != Some((conflicted, marked)) {
            self.bubble_reload.setEnabled(conflicted && !marked);
            self.bubble_rebase.setEnabled(conflicted && !marked);
            self.bubble_apply.setEnabled(!conflicted && !marked);
            self.bubble_control_state = Some((conflicted, marked));
            self.bubble_reload.setHidden(!conflicted);
            self.bubble_rebase.setHidden(!conflicted);
        }
        if self.bubble_conflict_label != Some(conflicted) {
            let label = if conflicted {
                text(self.locale, Message::BubbleColorDraftConflict)
            } else {
                text(self.locale, Message::CustomizeBubbleColors)
            };
            self.bubble_colors_label
                .setStringValue(&NSString::from_str(label));
            set_accessibility_label(&self.bubble_colors_label, label);
            self.bubble_conflict_label = Some(conflicted);
            self.layout_documents_at(reading_position.expect("changed conflict captures position"));
        }
        self.localize_detail_openers();
        self.end_control_refresh();
    }

    pub(crate) fn set_bubble_appearance(&mut self, appearance: BubbleAppearance) {
        let protected: [bool; 5] = std::array::from_fn(|index| {
            self.bubble_field_editor(index).is_some() || self.bubble_field_dirty(index)
        });
        let editing =
            protected.iter().any(|protected| *protected) || self.bubble_draft.conflicted();
        let theme_changed =
            self.bubble_draft.latest.map(|saved| saved.theme) != Some(appearance.theme);
        self.bubble_draft.receive(appearance, editing);
        if !editing {
            let tag = match appearance.theme {
                BubbleTheme::WarmIvory => 0,
                BubbleTheme::DustyRose => 1,
                BubbleTheme::MoonlitInk => 2,
                BubbleTheme::Custom => 3,
            };
            self.bubble_theme_popup.selectItemWithTag(tag);
        }
        if !editing {
            let palette = appearance.palette();
            for (field, value) in self.bubble_color_fields.iter().zip([
                palette.surface,
                palette.text,
                palette.muted,
                palette.border,
                palette.accent,
            ]) {
                let hex = value.to_hex();
                if field.stringValue().to_string() != hex {
                    field.setStringValue(&NSString::from_str(&hex));
                }
            }
        }
        self.update_color_swatches();
        self.refresh_bubble_color_controls();
        if theme_changed {
            self.layout_documents();
        }
    }

    pub(crate) fn bubble_colors_conflicted(&self) -> bool {
        self.bubble_draft.conflicted()
    }

    pub(crate) fn bubble_colors_saved(&mut self, appearance: BubbleAppearance) {
        self.bubble_draft.saved(appearance);
        let tag = match appearance.theme {
            BubbleTheme::WarmIvory => 0,
            BubbleTheme::DustyRose => 1,
            BubbleTheme::MoonlitInk => 2,
            BubbleTheme::Custom => 3,
        };
        self.bubble_theme_popup.selectItemWithTag(tag);
        self.refresh_bubble_color_controls();
        self.layout_documents();
    }

    pub(crate) fn reload_bubble_colors(&mut self) -> bool {
        if self.bubble_colors_marked() {
            return false;
        }
        let Some(appearance) = self.bubble_draft.latest else {
            return false;
        };
        self.bubble_draft.rebase();
        let tag = match appearance.theme {
            BubbleTheme::WarmIvory => 0,
            BubbleTheme::DustyRose => 1,
            BubbleTheme::MoonlitInk => 2,
            BubbleTheme::Custom => 3,
        };
        self.bubble_theme_popup.selectItemWithTag(tag);
        let palette = appearance.palette();
        for (field, value) in self.bubble_color_fields.iter().zip([
            palette.surface,
            palette.text,
            palette.muted,
            palette.border,
            palette.accent,
        ]) {
            let hex = value.to_hex();
            if field.stringValue().to_string() != hex {
                field.setStringValue(&NSString::from_str(&hex));
            }
        }
        self.update_color_swatches();
        self.refresh_bubble_color_controls();
        true
    }

    pub(crate) fn rebase_bubble_colors(&mut self) -> bool {
        if self.bubble_colors_marked() || !self.bubble_draft.rebase() {
            return false;
        }
        self.refresh_bubble_color_controls();
        true
    }

    pub(crate) fn set_show_status_indicators(&self, enabled: bool) {
        self.status_indicators_switch.setState(if enabled {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }

    pub(crate) fn custom_bubble_palette(&self) -> Result<BubblePalette, String> {
        if self.bubble_colors_conflicted() {
            return Err(text(self.locale, Message::BubbleColorDraftConflict).to_owned());
        }
        if self.bubble_colors_marked() {
            return Err(text(self.locale, Message::FinishMarkedText).to_owned());
        }
        let parse = |index: usize| -> Result<BubbleColor, String> {
            BubbleColor::parse_hex(&self.bubble_color_fields[index].stringValue().to_string())
        };
        Ok(BubblePalette {
            surface: parse(0)?,
            text: parse(1)?,
            muted: parse(2)?,
            border: parse(3)?,
            accent: parse(4)?,
        })
    }

    pub(crate) fn sync(
        &mut self,
        scene: &Scene,
        menu_bar_mode: MenuBarMode,
        language: LanguagePreference,
        status: &str,
        lifecycle: LifecycleSettings,
        observation: &ObservationPreferences,
        providers: &std::collections::BTreeMap<String, String>,
        connections: &HostConnections,
    ) {
        self.begin_control_refresh();
        self.character_visible_switch.setState(if scene.visible {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.bubble_visible_switch
            .setState(if scene.bubble_visible {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        set_action(
            &*self.character_visible_switch,
            &self.target,
            if scene.visible {
                sel!(hide:)
            } else {
                sel!(show:)
            },
        );

        set_action(
            &*self.bubble_visible_switch,
            &self.target,
            if scene.bubble_visible {
                sel!(hideBubble:)
            } else {
                sel!(showBubble:)
            },
        );
        let active_placement = placement_index(scene.bubble_placement);
        for (index, button) in self.placement_buttons.iter().enumerate() {
            button.setState(if index == active_placement {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }

        self.full_passthrough_switch.setState(if scene.passthrough {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.alpha_passthrough_switch
            .setState(if scene.alpha_passthrough {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });

        let scale = if scene.scale.is_finite() {
            scene.scale.clamp(MIN_SCALE, MAX_SCALE)
        } else {
            MIN_SCALE
        };
        self.scale_readout
            .setStringValue(&NSString::from_str(&format!("{scale:.2}×")));
        self.scale_down.setEnabled(scale > MIN_SCALE + f64::EPSILON);
        self.scale_up.setEnabled(scale < MAX_SCALE - f64::EPSILON);

        self.set_language_preference(language);
        self.set_menu_bar_mode(menu_bar_mode);
        if self.status_text != status {
            self.status_text.clear();
            self.status_text.push_str(status);
        }
        self.update_status_text();
        self.lifecycle_card.sync(lifecycle);
        self.sync_observation(observation, providers, connections);
        self.update_color_swatches();
        self.layout_root();
        self.refresh_lifecycle_diagnostics();
        self.end_control_refresh();
    }

    pub(crate) fn set_language_preference(&self, language: LanguagePreference) {
        let tag = match language {
            LanguagePreference::System => 0,
            LanguagePreference::Ko => 1,
            LanguagePreference::En => 2,
            LanguagePreference::ZhHans => 3,
            LanguagePreference::Ja => 4,
        };
        self.language_popup.selectItemWithTag(tag);
    }

    pub(crate) fn set_menu_bar_mode(&self, mode: MenuBarMode) {
        self.menu_bar_popup.selectItemWithTag(match mode {
            MenuBarMode::Always => 0,
            MenuBarMode::RecoveryOnly => 1,
        });
    }

    pub(crate) fn set_presentation_error(&mut self, error: Option<&str>) {
        self.presentation_error = error.map(str::to_owned);
        let color = if error.is_some() {
            NSColor::systemRedColor()
        } else {
            secondary()
        };
        self.status.setTextColor(Some(&color));
        self.update_status_text();
        self.layout_root();
    }

    fn update_status_text(&self) {
        let value = if self.presentation_error.is_some() {
            text(self.locale, Message::PresentationSaveFailure)
        } else {
            &self.status_text
        };
        set_readonly_value(&self.status, value);
        set_tooltip(&self.status, value);
        self.footer_details
            .setHidden(self.presentation_error.is_none());
        set_readonly_value(
            &self.appearance_error,
            self.presentation_error
                .as_deref()
                .unwrap_or(text(self.locale, Message::SettingsDetailsEmpty)),
        );
        self.localize_detail_openers();
    }

    pub(crate) fn set_menu_bar_icon(
        &mut self,
        image: &NSImage,
        custom: bool,
        error: Option<(Message, &str)>,
    ) {
        self.begin_control_refresh();
        self.menu_bar_preview.setImage(Some(image));
        self.menu_bar_icon_custom = custom;
        self.menu_bar_icon_error_detail =
            error.map(|(message, detail)| (message, detail.to_owned()));
        self.update_menu_bar_icon_labels();
        self.menu_bar_restore
            .setEnabled(custom && !self.menu_bar_icon_busy.get());
        self.layout_documents();
        self.end_control_refresh();
    }

    pub(crate) fn set_menu_bar_icon_busy(&mut self, busy: bool) {
        // This setter is called on the main thread; keep the state together
        // with the controls so clearing busy never enables reset for Default.
        self.begin_control_refresh();
        self.menu_bar_icon_busy.set(busy);
        self.menu_bar_choose.setEnabled(!busy);
        self.menu_bar_restore
            .setEnabled(self.menu_bar_icon_custom && !busy);
        self.end_control_refresh();
    }

    pub(crate) fn set_reset_settings_busy(&mut self, busy: bool) {
        self.begin_control_refresh();
        self.reset_settings_busy = busy;
        self.reset_settings.setEnabled(!busy);
        self.end_control_refresh();
    }

    pub(crate) fn set_settings_recovery_status(&self, status: &str) {
        set_readonly_value(&self.settings_recovery_status, status);
        set_tooltip(&self.settings_recovery_status, status);
        set_accessibility_label(&self.settings_recovery_status, status);
        self.settings_recovery_status.setHidden(status.is_empty());
        self.layout_detail();
    }

    pub(crate) fn reset_settings_enabled(&self) -> bool {
        !self.update_frozen
            && !self.reset_settings_busy
            && !self.menu_bar_icon_busy.get()
            && !self.bubble_colors_marked()
    }

    fn update_menu_bar_icon_labels(&self) {
        self.menu_bar_icon_status
            .setStringValue(&NSString::from_str(text(
                self.locale,
                if self.menu_bar_icon_custom {
                    Message::MenuBarIconCustom
                } else {
                    Message::MenuBarIconDefault
                },
            )));
        if let Some((summary, detail)) = &self.menu_bar_icon_error_detail {
            let value = if detail.is_empty() {
                text(self.locale, *summary).to_owned()
            } else {
                format!("{}: {detail}", text(self.locale, *summary))
            };
            self.menu_bar_icon_error
                .setStringValue(&NSString::from_str(&value));
            self.menu_bar_icon_error.setHidden(false);
        } else {
            self.menu_bar_icon_error
                .setStringValue(&NSString::from_str(""));
            self.menu_bar_icon_error.setHidden(true);
        }
        self.localize_detail_openers();
    }

    pub(crate) fn revert_observation_controls(&mut self) {
        self.observation_locale = None;
    }

    pub(crate) fn sync_observation(
        &mut self,
        preferences: &ObservationPreferences,
        providers: &std::collections::BTreeMap<String, String>,
        connections: &HostConnections,
    ) {
        if self.observation_preferences.as_ref() == Some(preferences)
            && self.provider_labels == *providers
            && self.connection_hosts == *connections
            && self.observation_locale == Some(self.locale)
        {
            return;
        }
        self.begin_control_refresh();
        self.observation_locale = Some(self.locale);
        self.provider_labels.clone_from(providers);
        self.connection_hosts.clone_from(connections);
        self.observation_preferences = Some(preferences.clone());
        self.rebuild_observation_controls();
        self.layout_documents();
        self.end_control_refresh();
    }

    fn rebuild_observation_controls(&mut self) {
        let Some(preferences) = self.observation_preferences.as_ref() else {
            return;
        };
        self.provider_ids = self.provider_labels.keys().cloned().collect();
        self.connection_host_popup.removeAllItems();
        self.connection_host_popup
            .addItemWithTitle(&NSString::from_str(text(
                self.locale,
                Message::ConnectionHostAuto,
            )));
        if let Some(item) = self.connection_host_popup.itemAtIndex(0) {
            item.setTag(0);
        }
        for (index, provider) in self.provider_ids.iter().enumerate() {
            self.connection_host_popup
                .addItemWithTitle(&NSString::from_str(&self.provider_labels[provider]));
            if let Some(item) = self.connection_host_popup.itemAtIndex((index + 1) as isize) {
                item.setTag((index + 1) as isize);
            }
        }
        if let ObservationSelection::Provider(provider) = &preferences.host {
            if !self.provider_labels.contains_key(provider) {
                self.provider_ids.push(provider.clone());
                self.connection_host_popup
                    .addItemWithTitle(&NSString::from_str(text(
                        self.locale,
                        Message::ConnectionUnavailable,
                    )));
                if let Some(item) = self
                    .connection_host_popup
                    .itemAtIndex(self.provider_ids.len() as isize)
                {
                    item.setTag(self.provider_ids.len() as isize);
                }
            }
        }
        let tag = match &preferences.host {
            ObservationSelection::Auto => 0,
            ObservationSelection::Provider(provider) => self
                .provider_ids
                .iter()
                .position(|id| id == provider)
                .map_or(-1, |index| (index + 1) as isize),
        };
        self.connection_host_popup.selectItemWithTag(tag);
        let mut status = String::new();
        for (provider, label) in &self.provider_labels {
            let count = self
                .connection_hosts
                .get(provider)
                .copied()
                .unwrap_or_default();
            let message = if !preferences.host.includes_provider(provider) {
                Message::ConnectionDisabled
            } else if count > 0 {
                Message::ConnectionConnected
            } else {
                Message::ConnectionWaiting
            };
            if !status.is_empty() {
                status.push('\n');
            }
            let _ = write!(status, "{label}: {} · {count}", text(self.locale, message));
        }
        if self.provider_labels.is_empty() {
            status.push_str(text(self.locale, Message::ConnectionNoProviders));
        }
        if matches!(&preferences.host, ObservationSelection::Provider(provider) if !self.provider_labels.contains_key(provider))
        {
            if !status.is_empty() {
                status.push('\n');
            }
            status.push_str(text(self.locale, Message::ConnectionUnavailable));
        }
        set_readonly_value(&self.connection_status, &status);
        self.update_connection_summary();
    }

    pub(crate) fn observation_selection(&self, tag: isize) -> Option<ObservationSelection> {
        if tag == 0 {
            return Some(ObservationSelection::Auto);
        }
        self.provider_ids
            .get(usize::try_from(tag.checked_sub(1)?).ok()?)
            .cloned()
            .map(ObservationSelection::Provider)
    }

    pub(crate) fn sync_integration_settings(
        &mut self,
        settings: &[(String, LifecycleSettingView)],
        actions: &[(String, desktop_pet_contracts::LifecycleActionView)],
        error: Option<&str>,
    ) {
        self.begin_control_refresh();
        self.lifecycle_card.sync_extensions(settings);
        self.lifecycle_card.sync_actions(actions);
        self.lifecycle_card.set_extensions_available(true);
        self.connection_snapshot_unavailable = false;
        self.connection_query_error = error.map(str::to_owned);
        self.refresh_connection_error();
        self.layout_documents();
        self.end_control_refresh();
    }

    pub(crate) fn extension_setting_enabled(&self, provider: &str, key: &str) -> bool {
        !self.update_frozen
            && !self.connection_snapshot_unavailable
            && self.lifecycle_card.extension_setting_enabled(provider, key)
    }
    pub(crate) fn extension_action_enabled(&self, provider: &str, key: &str) -> bool {
        !self.update_frozen
            && !self.connection_snapshot_unavailable
            && self.lifecycle_card.extension_action_enabled(provider, key)
    }
    pub(crate) fn set_connection_operation_error(&mut self, detail: &str) {
        self.connection_operation_error = Some(detail.to_owned());
        self.refresh_connection_error();
        self.layout_documents();
    }
    pub(crate) fn clear_connection_operation_error(&mut self) {
        self.connection_operation_error = None;
        self.refresh_connection_error();
        self.layout_documents();
    }
    fn connection_has_error(&self) -> bool {
        self.connection_query_error.is_some() || self.connection_operation_error.is_some()
    }

    pub(crate) fn set_connection_snapshot_unavailable(&mut self, detail: &str) {
        self.begin_control_refresh();
        self.connection_snapshot_unavailable = true;
        self.connection_query_error = Some(detail.to_owned());
        self.lifecycle_card.set_extensions_available(false);
        self.refresh_connection_error();
        self.layout_documents();
        self.end_control_refresh();
    }
    fn update_connection_summary(&self) {
        let Some(preferences) = &self.observation_preferences else {
            set_readonly_value(
                &self.connection_selection_summary,
                text(self.locale, Message::ConnectionHostAuto),
            );
            set_readonly_value(
                &self.connection_summary,
                text(self.locale, Message::SettingsSnapshotUnavailable),
            );
            return;
        };
        let name = match &preferences.host {
            ObservationSelection::Auto => text(self.locale, Message::ConnectionHostAuto),
            ObservationSelection::Provider(provider) => self
                .provider_labels
                .get(provider)
                .map(String::as_str)
                .unwrap_or(text(self.locale, Message::ConnectionUnavailable)),
        };
        set_readonly_value(&self.connection_selection_summary, name);
        let count = connection_selected_count(
            &preferences.host,
            &self.provider_labels,
            &self.connection_hosts,
        );
        let message = connection_summary_message(
            &preferences.host,
            &self.provider_labels,
            &self.connection_hosts,
        );
        set_readonly_value(
            &self.connection_summary,
            &format!("{} · {count}", text(self.locale, message)),
        );
    }
    fn refresh_connection_error(&self) {
        let combined = match (
            &self.connection_query_error,
            &self.connection_operation_error,
        ) {
            (Some(query), Some(operation)) => Some(format!("{query}\n\n{operation}")),
            _ => None,
        };
        let detail = combined
            .as_deref()
            .or(self.connection_query_error.as_deref())
            .or(self.connection_operation_error.as_deref())
            .unwrap_or(text(self.locale, Message::SettingsDetailsEmpty));
        set_readonly_value(&self.observation_error, detail);
        set_readonly_value(&self.connection_settings_error, detail);
        let message = connection_query_message(
            self.connection_snapshot_unavailable,
            self.connection_has_error(),
        )
        .unwrap_or(Message::SettingsConnectionError);
        set_readonly_value(&self.connection_error_summary, text(self.locale, message));
        self.localize_detail_openers();
    }
    fn refresh_lifecycle_diagnostics(&self) {
        set_readonly_value(
            &self.lifecycle_error_detail,
            self.lifecycle_card
                .error_detail()
                .unwrap_or(text(self.locale, Message::SettingsDetailsEmpty)),
        );
        self.localize_detail_openers();
    }
    pub(crate) fn clear_connection_profile(&mut self) {
        for field in &self.connection_profile_values {
            set_readonly_value(field, text(self.locale, Message::SettingsDetailsEmpty));
            set_tooltip(field, "");
        }
        self.localize_connection();
        self.layout_detail();
    }

    pub(crate) fn set_connection_profile(
        &mut self,
        executable: &std::path::Path,
        config: &std::path::Path,
        state: &std::path::Path,
        runtime_identity: &str,
    ) {
        let control_socket = state.join("control.sock");
        for (field, path) in self.connection_profile_values.iter().zip([
            executable,
            config,
            state,
            control_socket.as_path(),
        ]) {
            let value = path.to_string_lossy();
            set_readonly_value(field, &value);
            set_tooltip(field, &value);
        }
        let field = &self.connection_profile_values[4];
        set_readonly_value(field, runtime_identity);
        set_tooltip(field, runtime_identity);
        self.localize_connection();
        self.layout_documents();
    }

    fn localize_connection(&self) {
        let locale = self.locale;
        for (field, message) in [
            (&self.connection_host_label, Message::ConnectionHost),
            (&self.connection_auto_help, Message::ConnectionAutoHelp),
            (&self.connection_profile_title, Message::ConnectionProfile),
        ] {
            let title = text(locale, message);
            field.setStringValue(&NSString::from_str(title));
            set_accessibility_label(field, title);
        }
        localize_popup_items(
            &self.connection_host_popup,
            locale,
            &[(Message::ConnectionHostAuto, 0)],
        );
        set_accessibility_label(
            &self.connection_host_popup,
            text(locale, Message::ConnectionHost),
        );
        for ((title, value), message) in self
            .connection_profile_labels
            .iter()
            .zip(&self.connection_profile_values)
            .zip([
                Message::ConnectionExecutable,
                Message::ConnectionConfigDirectory,
                Message::ConnectionStateDirectory,
                Message::ConnectionControlSocket,
                Message::ConnectionRuntimeIdentity,
            ])
        {
            let name = text(locale, message);
            title.setStringValue(&NSString::from_str(name));
            set_accessibility_label(value, &format!("{name}: {}", value.stringValue()));
        }
    }

    pub(crate) fn set_locale(&mut self, locale: UiLocale) {
        self.begin_control_refresh();
        self.panel
            .setTitle(&NSString::from_str(text(locale, Message::MenuPanelTitle)));
        self.panel_title
            .setStringValue(&NSString::from_str(text(locale, Message::MenuPanelTitle)));
        set_accessibility_label(&*self.panel_title, text(locale, Message::MenuPanelTitle));
        for (button, message) in self.tabs.iter().zip([
            Message::MenuAppearanceTab,
            Message::MenuConnectionsTab,
            Message::MenuAppTab,
        ]) {
            button.setTitle(&NSString::from_str(text(locale, message)));
            set_accessibility_label(button, text(locale, message));
        }
        self.character_visible_label
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuCharacterVisible,
            )));
        self.character_browser_open
            .setTitle(&NSString::from_str(text(
                locale,
                Message::CharacterBrowserOpen,
            )));
        set_accessibility_label(
            &self.character_browser_open,
            text(locale, Message::CharacterBrowserOpen),
        );
        set_tooltip(
            &self.character_browser_open,
            text(locale, Message::CharacterBrowserOpen),
        );
        let browser_symbol = NSString::from_str("square.grid.2x2");
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &browser_symbol,
            Some(&NSString::from_str(text(
                locale,
                Message::CharacterBrowserOpen,
            ))),
        ) {
            self.character_browser_open.setImage(Some(&image));
        }
        self.character_editing_open
            .setTitle(&NSString::from_str(text(
                locale,
                Message::CharacterEditingOpen,
            )));
        set_accessibility_label(
            &self.character_editing_open,
            text(locale, Message::CharacterEditingOpen),
        );
        set_tooltip(
            &self.character_editing_open,
            text(locale, Message::CharacterEditingOpen),
        );
        let symbol_name = NSString::from_str("bubble.left.and.text.bubble.right");
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &symbol_name,
            Some(&NSString::from_str(text(
                locale,
                Message::CharacterEditingOpen,
            ))),
        ) {
            self.character_editing_open.setImage(Some(&image));
        }
        self.bubble_visible_label
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuBubbleVisible,
            )));
        self.status_indicators_label
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuStatusIndicators,
            )));
        self.bubble_placement_label
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuBubblePlacement,
            )));
        let placement_titles = [
            Message::MenuPlacementAuto,
            Message::MenuPlacementAbove,
            Message::MenuPlacementBelow,
            Message::MenuPlacementLeft,
            Message::MenuPlacementRight,
        ];
        for (button, message) in self.placement_buttons.iter().zip(placement_titles) {
            let title = text(locale, message);
            button.setTitle(&NSString::from_str(title));
            set_accessibility_label(&*button, title);
        }
        self.bubble_theme_label
            .setStringValue(&NSString::from_str(text(locale, Message::BubbleTheme)));
        localize_popup_items(
            &self.bubble_theme_popup,
            locale,
            &[
                (Message::ThemeWarmIvory, 0),
                (Message::ThemeDustyRose, 1),
                (Message::ThemeMoonlitInk, 2),
                (Message::ThemeCustom, 3),
            ],
        );
        set_accessibility_label(&self.bubble_theme_popup, text(locale, Message::BubbleTheme));
        for ((name, field), message) in self
            .bubble_color_labels
            .iter()
            .zip(&self.bubble_color_fields)
            .zip([
                Message::BubbleSurfaceColor,
                Message::BubbleTextColor,
                Message::BubbleMutedColor,
                Message::BubbleBorderColor,
                Message::BubbleAccentColor,
            ])
        {
            name.setStringValue(&NSString::from_str(text(locale, message)));
            set_accessibility_label(field, text(locale, message));
        }
        self.bubble_apply.setTitle(&NSString::from_str(text(
            locale,
            Message::ApplyBubbleColors,
        )));
        self.bubble_reset.setTitle(&NSString::from_str(text(
            locale,
            Message::ResetBubbleColors,
        )));
        set_accessibility_label(&self.bubble_apply, text(locale, Message::ApplyBubbleColors));
        set_accessibility_label(&self.bubble_reset, text(locale, Message::ResetBubbleColors));
        for (button, message) in [
            (&self.bubble_reload, Message::BubbleColorsReload),
            (&self.bubble_rebase, Message::BubbleColorsRebase),
        ] {
            button.setTitle(&NSString::from_str(text(locale, message)));
            set_accessibility_label(button, text(locale, message));
        }
        self.settings_appearance_label
            .setStringValue(&NSString::from_str(text(locale, Message::MenuAppearance)));
        self.click_behavior_label
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuClickBehavior,
            )));
        self.full_passthrough_label
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuFullPassthrough,
            )));
        self.full_passthrough_help
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuFullPassthroughHelp,
            )));
        self.alpha_passthrough_label
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuAlphaPassthrough,
            )));
        self.alpha_passthrough_help
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuAlphaPassthroughHelp,
            )));
        self.scale_label
            .setStringValue(&NSString::from_str(text(locale, Message::MenuScale)));
        self.language_label
            .setStringValue(&NSString::from_str(text(locale, Message::MenuLanguage)));
        self.lifecycle_card.set_locale(locale);
        self.update_card.set_locale(locale);
        localize_popup_items(
            &self.language_popup,
            locale,
            &[
                (Message::LanguageSystem, 0),
                (Message::KoreanLanguage, 1),
                (Message::EnglishLanguage, 2),
                (Message::SimplifiedChineseLanguage, 3),
                (Message::JapaneseLanguage, 4),
            ],
        );
        set_accessibility_label(&self.language_popup, text(locale, Message::MenuLanguage));
        set_tooltip(
            &self.language_popup,
            text(locale, Message::FollowSystemSettings),
        );
        self.language_popup
            .menu()
            .expect("language popup menu")
            .itemWithTag(0)
            .expect("system language item")
            .setToolTip(Some(&NSString::from_str(text(
                locale,
                Message::FollowSystemSettings,
            ))));
        self.menu_bar_label
            .setStringValue(&NSString::from_str(text(locale, Message::MenuGeneral)));
        set_accessibility_label(&*self.menu_bar_label, text(locale, Message::MenuGeneral));
        self.menu_bar_visibility
            .setStringValue(&NSString::from_str(text(
                locale,
                Message::MenuBarVisibility,
            )));
        self.menu_bar_help
            .setStringValue(&NSString::from_str(text(locale, Message::MenuBarModeHelp)));
        self.menu_bar_icon_help
            .setStringValue(&NSString::from_str(text(locale, Message::MenuBarIconHelp)));
        self.menu_bar_choose.setTitle(&NSString::from_str(text(
            locale,
            Message::MenuBarIconChoose,
        )));
        self.menu_bar_restore.setTitle(&NSString::from_str(text(
            locale,
            Message::MenuBarIconRestore,
        )));
        set_accessibility_label(
            &*self.menu_bar_choose,
            text(locale, Message::MenuBarIconChoose),
        );
        set_accessibility_label(
            &*self.menu_bar_restore,
            text(locale, Message::MenuBarIconRestore),
        );
        localize_popup_items(
            &self.menu_bar_popup,
            locale,
            &[
                (Message::MenuBarAlways, 0),
                (Message::MenuBarRecoveryOnly, 1),
            ],
        );
        set_accessibility_label(
            &self.menu_bar_popup,
            text(locale, Message::MenuBarVisibility),
        );
        set_accessibility_label(&*self.menu_bar_help, text(locale, Message::MenuBarModeHelp));
        set_tooltip(&self.menu_bar_popup, text(locale, Message::MenuBarModeHelp));
        for tag in [0, 1] {
            self.menu_bar_popup
                .menu()
                .expect("menu bar mode popup menu")
                .itemWithTag(tag)
                .expect("menu bar mode item")
                .setToolTip(Some(&NSString::from_str(text(
                    locale,
                    Message::MenuBarModeHelp,
                ))));
        }
        self.locale = locale;
        self.bubble_conflict_label = None;
        self.refresh_bubble_color_controls();
        self.update_menu_bar_icon_labels();
        self.observation_locale = None;
        self.rebuild_observation_controls();
        self.observation_title
            .setStringValue(&NSString::from_str(text(locale, Message::ConnectionTitle)));
        self.localize_connection();
        self.reset
            .setTitle(&NSString::from_str(text(locale, Message::ResetPosition)));
        self.restart
            .setTitle(&NSString::from_str(text(locale, Message::MenuRestartApp)));
        self.quit
            .setTitle(&NSString::from_str(text(locale, Message::MenuQuit)));
        self.reset_settings
            .setTitle(&NSString::from_str(text(locale, Message::ResetAppSettings)));
        set_accessibility_label(
            &self.reset_settings,
            text(locale, Message::ResetAppSettings),
        );
        for (field, message) in [
            (&self.recovery_reset_help, Message::AppRecoveryResetHelp),
            (
                &self.recovery_preserved_help,
                Message::AppRecoveryPreservedHelp,
            ),
        ] {
            set_readonly_value(field, text(locale, message));
            set_accessibility_label(field, text(locale, message));
        }
        set_accessibility_label(&*self.scale_down, text(locale, Message::ScaleDown));
        set_accessibility_label(&*self.scale_up, text(locale, Message::ScaleUp));
        set_accessibility_label(
            &*self.character_visible_switch,
            text(locale, Message::MenuCharacterVisible),
        );
        set_accessibility_label(
            &*self.bubble_visible_switch,
            text(locale, Message::MenuBubbleVisible),
        );
        set_accessibility_label(
            &*self.status_indicators_switch,
            text(locale, Message::MenuStatusIndicators),
        );
        set_accessibility_label(
            &*self.full_passthrough_switch,
            text(locale, Message::MenuFullPassthrough),
        );
        set_accessibility_label(
            &*self.alpha_passthrough_switch,
            text(locale, Message::MenuAlphaPassthrough),
        );
        set_accessibility_label(&*self.reset, text(locale, Message::ResetPosition));
        set_accessibility_label(&*self.restart, text(locale, Message::MenuRestartApp));
        set_accessibility_label(&*self.quit, text(locale, Message::MenuQuit));
        set_tooltip(&self.quit, text(locale, Message::LifecycleQuitHelp));
        let _: () = unsafe {
            msg_send![&*self.quit, setAccessibilityHelp: Some(&*NSString::from_str(text(locale, Message::LifecycleQuitHelp)))]
        };
        self.update_status_text();
        self.update_connection_summary();
        self.refresh_connection_error();
        self.refresh_lifecycle_diagnostics();
        self.localize_detail_openers();
        self.end_control_refresh();
        self.layout_root();
    }

    pub(crate) fn reanchor_at(&self, anchor_screen_rect: NSRect, visible_frame: NSRect) {
        let height = PANEL_HEIGHT.min((visible_frame.size.height - 16.0).max(1.0));
        let width = PANEL_WIDTH.min((visible_frame.size.width - 16.0).max(1.0));
        self.panel.setContentSize(NSSize::new(width, height));
        self.root.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, height),
        ));
        self.layout_root();

        let panel_size = self.panel.frame().size;
        let mut x =
            anchor_screen_rect.origin.x + (anchor_screen_rect.size.width - panel_size.width) * 0.5;
        let below = anchor_screen_rect.origin.y - panel_size.height - 6.0;
        let above = anchor_screen_rect.origin.y + anchor_screen_rect.size.height + 6.0;
        let y = if below >= visible_frame.origin.y {
            below
        } else {
            above
        };
        x = x.clamp(
            visible_frame.origin.x,
            (visible_frame.origin.x + visible_frame.size.width - panel_size.width)
                .max(visible_frame.origin.x),
        );
        let y = y.clamp(
            visible_frame.origin.y,
            (visible_frame.origin.y + visible_frame.size.height - panel_size.height)
                .max(visible_frame.origin.y),
        );
        self.panel.setFrameOrigin(NSPoint::new(x, y));
    }

    pub(crate) fn shutdown(&self) {
        self.hide();
        self.panel.close();
        self.detail_window.finish_shutdown();
    }

    pub(crate) fn window(&self) -> &NSPanel {
        &self.panel
    }
    pub(crate) fn lifecycle_value(&self, key: crate::lifecycle::LifecycleSetting) -> bool {
        self.lifecycle_card.value(key)
    }
    pub(crate) fn refresh_app_update(&mut self, model: &UpdateCardModel) {
        self.begin_control_refresh();
        self.update_card.refresh(model);
        self.update_details_problem = model.status
            == desktop_pet_update_coordinator::CheckStatus::Failed
            || matches!(
                model.primary_action,
                Some(
                    crate::update_card::UpdateCardPrimaryAction::Recover
                        | crate::update_card::UpdateCardPrimaryAction::RecoverStart
                        | crate::update_card::UpdateCardPrimaryAction::RecoverStoppedOriginal
                )
            );
        self.localize_detail_openers();
        self.layout_root();
        self.layout_detail();
        self.end_control_refresh();
    }
    fn restore_frozen_controls(&mut self) {
        for (control, enabled) in self.frozen_controls.drain(..) {
            control.setEnabled(enabled);
        }
    }
    fn begin_control_refresh(&mut self) {
        if self.control_refresh_depth == 0 && self.update_frozen {
            self.restore_frozen_controls();
        }
        self.control_refresh_depth += 1;
    }
    fn end_control_refresh(&mut self) {
        self.control_refresh_depth -= 1;
        if self.control_refresh_depth == 0 && self.update_frozen {
            self.freeze_controls();
        }
    }
    fn freeze_controls(&mut self) {
        fn visit(
            view: &NSView,
            exempt: &[&NSView],
            controls: &mut Vec<(Retained<NSControl>, bool)>,
        ) {
            for child in view.subviews().iter() {
                if let Some(control) = child.downcast_ref::<NSControl>() {
                    let readonly = child
                        .downcast_ref::<NSTextField>()
                        .is_some_and(|field| !field.isEditable());
                    let opener = child
                        .downcast_ref::<NSButton>()
                        .is_some_and(|button| button.action() == Some(sel!(openSettingsDetail:)));
                    let recovery = child.downcast_ref::<NSButton>().is_some_and(|button| {
                        matches!(button.action(), Some(action) if
                            action == sel!(recoverAppUpdate:)
                                || action == sel!(showAppUpdateRecoveryProgress:))
                    });
                    let scroller = child.downcast_ref::<NSScroller>().is_some();
                    let editing = child
                        .downcast_ref::<SettingsColorNativeField>()
                        .is_some_and(|field| field.ivars().get());
                    if !readonly
                        && !opener
                        && !recovery
                        && !scroller
                        && !editing
                        && !exempt.iter().any(|view| std::ptr::eq(*view, &*child))
                    {
                        controls.push((control.retain(), control.isEnabled()));
                        control.setEnabled(false);
                    }
                }
                visit(&child, exempt, controls);
            }
        }
        // Neither setEnabled(false) on the host nor setEditable(false) on
        // its field editor may run here: both can end an IME session.
        // The owned editor rejects mutations without changing that session.
        for index in 0..self.bubble_color_fields.len() {
            if let Some(editor) = self
                .bubble_field_editor(index)
                .and_then(|editor| editor.downcast_ref::<NSTextView>())
            {
                self.bubble_color_fields[index].ivars().set(true);
                if let Some(editor) = editor.downcast_ref::<SettingsFieldEditor>() {
                    editor.ivars().set(true);
                }
            }
        }
        let exempt: [&NSView; 4] = [&self.quit, &self.tabs[0], &self.tabs[1], &self.tabs[2]];
        visit(&self.root, &exempt, &mut self.frozen_controls);
        visit(&self.detail_root, &exempt, &mut self.frozen_controls);
    }
    pub(crate) fn set_update_frozen(&mut self, frozen: bool) {
        if self.update_frozen == frozen {
            return;
        }
        self.update_frozen = frozen;
        self.detail_window.ivars().update_frozen.set(frozen);
        if let Some(editor) = self.detail_window.ivars().field_editor.get() {
            editor.ivars().set(frozen);
        }
        if frozen {
            self.freeze_controls();
        } else {
            for field in &self.bubble_color_fields {
                field.ivars().set(false);
            }
            self.restore_frozen_controls();
        }
    }

    #[cfg(test)]
    pub(crate) fn regression_color_editor(&mut self) -> Retained<NSTextView> {
        let window = self.prepare_detail(SettingsDetailPage::Appearance)
            .expect("Appearance detail could not open");
        window.makeKeyAndOrderFront(None);
        assert!(window.makeFirstResponder(Some(&*self.bubble_color_fields[0])));
        self.bubble_field_editor(0).and_then(|editor| editor.downcast_ref::<NSTextView>())
            .expect("real Settings color field editor is unavailable").retain()
    }

    #[cfg(test)]
    pub(crate) fn regression_appearance_controls(
        &self,
    ) -> (Retained<NSSwitch>, Retained<NSSwitch>, Retained<NSTextField>, Retained<NSScrollView>) {
        (
            self.full_passthrough_switch.retain(),
            self.alpha_passthrough_switch.retain(),
            self.scale_readout.retain(),
            self.detail_scroll.retain(),
        )
    }

    #[cfg(test)]
    pub(crate) fn regression_selected_placement(&self, placement: BubblePlacement) -> bool {
        self.placement_buttons.iter().enumerate().all(|(index, button)| {
            (button.state() == NSControlStateValueOn) == (index == placement_index(placement))
        })
    }

    pub(crate) fn bubble_restart_draft(&self, saved: BubbleAppearance) -> bool {
        if self.bubble_draft.conflicted()
            || self.bubble_colors_marked()
            || self.bubble_draft.baseline != Some(saved)
        {
            return true;
        }
        let expected_tag = match saved.theme {
            BubbleTheme::WarmIvory => 0,
            BubbleTheme::DustyRose => 1,
            BubbleTheme::MoonlitInk => 2,
            BubbleTheme::Custom => 3,
        };
        if self
            .bubble_theme_popup
            .selectedItem()
            .map(|item| item.tag())
            != Some(expected_tag)
        {
            return true;
        }
        let palette = saved.palette();
        let colors = [
            palette.surface,
            palette.text,
            palette.muted,
            palette.border,
            palette.accent,
        ];
        self.bubble_color_fields
            .iter()
            .zip(colors)
            .any(|(field, color)| {
                let raw = field.stringValue().to_string();
                BubbleColor::parse_hex(&raw).map_or(true, |actual| actual != color)
            })
    }

    pub(crate) fn lifecycle_saved(&mut self, settings: LifecycleSettings) {
        self.lifecycle_card.saved(settings);
        self.refresh_lifecycle_diagnostics();
        self.layout_documents();
    }

    pub(crate) fn lifecycle_failed(&mut self, settings: LifecycleSettings, detail: String) {
        self.lifecycle_card.failed(settings, detail);
        self.refresh_lifecycle_diagnostics();
        self.layout_documents();
    }

    fn apply_tab_selection(&self) {
        for (index, button) in self.tabs.iter().enumerate() {
            let selected = index == self.selected_tab.index();
            button.setState(if selected {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            let selected_color = if selected { primary() } else { secondary() };
            button.setContentTintColor(Some(&selected_color));
            let font = if selected {
                NSFont::boldSystemFontOfSize(11.5)
            } else {
                NSFont::systemFontOfSize(11.0)
            };
            button.setFont(Some(&font));
            set_accessibility_selected(&*button, selected);
        }
        self.update_tab_indicator();
    }

    fn update_tab_indicator(&self) {
        let bounds = self.root.bounds();
        let width = bounds.size.width.max(1.0);
        let track_x = 12.0;
        let track_width = (width - 24.0).max(1.0);
        let tab_width = (track_width / 3.0).max(1.0);
        let indicator_x = track_x + 2.0 + tab_width * self.selected_tab.index() as f64;
        self.tab_indicator.setFrame(NSRect::new(
            NSPoint::new(indicator_x, 50.0),
            NSSize::new((tab_width - 4.0).max(1.0), 26.0),
        ));
    }

    fn update_color_swatches(&self) {
        for (swatch, field) in self
            .bubble_color_swatches
            .iter()
            .zip(&self.bubble_color_fields)
        {
            let val = field.stringValue().to_string();
            if let Ok(color_val) = BubbleColor::parse_hex(&val) {
                let (r, g, b) = color_val.rgb();
                let ns_color = color(r, g, b, 1.0);
                swatch.setFillColor(&ns_color);
            }
        }
    }

    fn layout_root(&self) {
        self.layout_root_at(self.reading_position());
    }

    fn layout_root_at(&self, reading_position: ScrollReadingPosition) {
        let bounds = self.root.bounds();
        let width = bounds.size.width.max(1.0);
        let height = bounds.size.height.max(1.0);
        self.title.setFrame(NSRect::new(
            NSPoint::new(16.0, 10.0),
            NSSize::new((width - 32.0).max(1.0), 20.0),
        ));
        self.panel_title.setFrame(NSRect::new(
            NSPoint::new(16.0, 28.0),
            NSSize::new((width - 32.0).max(1.0), 16.0),
        ));

        let track_x = 12.0;
        let track_width = (width - 24.0).max(1.0);
        self.tab_track.setFrame(NSRect::new(
            NSPoint::new(track_x, 48.0),
            NSSize::new(track_width, 30.0),
        ));

        let tab_width = (track_width / 3.0).max(1.0);
        for (index, button) in self.tabs.iter().enumerate() {
            button.setFrame(NSRect::new(
                NSPoint::new(track_x + tab_width * index as f64, 48.0),
                NSSize::new(tab_width, 30.0),
            ));
        }
        self.update_tab_indicator();

        let scroll_height = (height - SCROLL_TOP - FOOTER_HEIGHT).max(1.0);
        self.scroll.setFrame(NSRect::new(
            NSPoint::new(PANEL_EDGE_INSET, SCROLL_TOP),
            NSSize::new((width - PANEL_EDGE_INSET * 2.0).max(1.0), scroll_height),
        ));
        self.layout_documents_at(reading_position);

        let footer_y = (height - FOOTER_HEIGHT + 10.0)
            .min((height - FOOTER_HEIGHT).max(SCROLL_TOP + 8.0))
            .max(SCROLL_TOP + 8.0);
        let button_h = 28.0;
        let quit_w = 48.0;
        let restart_w = 112.0;
        let details_w = if self.presentation_error.is_some() {
            70.0
        } else {
            0.0
        };
        let right_margin = 14.0;
        let quit_x = width - right_margin - quit_w;
        let restart_x = quit_x - 8.0 - restart_w;
        let status_w = (restart_x - details_w - 28.0).max(1.0);

        self.status.setFrame(NSRect::new(
            NSPoint::new(16.0, footer_y),
            NSSize::new(status_w, 32.0),
        ));
        self.footer_details.setFrame(NSRect::new(
            NSPoint::new(restart_x - details_w - 8.0, footer_y + 2.0),
            NSSize::new(details_w, button_h),
        ));
        self.restart.setFrame(NSRect::new(
            NSPoint::new(restart_x, footer_y + 2.0),
            NSSize::new(restart_w, button_h),
        ));
        self.quit.setFrame(NSRect::new(
            NSPoint::new(quit_x, footer_y + 2.0),
            NSSize::new(quit_w, button_h),
        ));
    }

    fn reading_position(&self) -> ScrollReadingPosition {
        let offset = self.scroll.contentView().bounds().origin.y;
        ScrollReadingPosition {
            offset,
            anchor: scroll_reading_anchor(&self.document, &self.document, offset),
        }
    }

    fn layout_documents(&self) {
        self.layout_documents_at(self.reading_position());
    }

    fn layout_documents_at(&self, reading_position: ScrollReadingPosition) {
        let width = self.scroll.contentView().bounds().size.width.max(1.0);
        let card_width = (width - 30.0).max(1.0);
        let viewport = self.scroll.contentView().bounds().size.height.max(1.0);
        let heights = [
            self.layout_appearance_basic(card_width, width),
            self.layout_connections_basic(card_width),
            self.layout_app_basic(card_width),
        ];
        for (index, view) in [&self.character_tab, &self.bubble_tab, &self.settings_tab]
            .iter()
            .enumerate()
        {
            view.setFrame(rect(0.0, 0.0, width, heights[index].max(viewport)));
        }
        self.document.setFrame(rect(
            0.0,
            0.0,
            width,
            heights[self.selected_tab.index()].max(viewport),
        ));
        let maximum = (self.document.frame().size.height - viewport).max(0.0);
        let offset = reading_position
            .anchor
            .filter(|(view, _)| {
                let hidden: bool = unsafe { msg_send![&**view, isHiddenOrHasHiddenAncestor] };
                view.window().is_some() && !hidden
            })
            .map_or(reading_position.offset, |(view, old_y)| {
                reading_position.offset
                    + view
                        .convertRect_toView(view.bounds(), Some(&self.document))
                        .origin
                        .y
                    - old_y
            })
            .clamp(0.0, maximum);
        self.scroll
            .contentView()
            .scrollToPoint(NSPoint::new(0.0, offset));
        self.scroll
            .reflectScrolledClipView(&self.scroll.contentView());
        self.panel.recalculateKeyViewLoop();
        self.layout_detail();
    }

    fn layout_appearance_basic(&self, width: f64, clip_width: f64) -> f64 {
        let content = (width - 28.0).max(1.0);
        let mut y = 8.0;
        self.character_view
            .setFrame(rect(0.0, y, clip_width, self.character_content_height));
        y += self.character_content_height + 8.0;
        self.character_browser_open
            .setFrame(rect(8.0, y, width, 30.0));
        y += 38.0;
        self.character_card.setFrame(rect(8.0, y, width, 46.0));
        place_switch(
            &self.character_visible_label,
            &self.character_visible_switch,
            width,
            8.0,
        );
        y += 54.0;
        self.scale_card.setFrame(rect(8.0, y, width, 50.0));
        self.scale_label
            .setFrame(rect(14.0, 14.0, (width - 154.0).max(1.0), 22.0));
        self.scale_down
            .setFrame(rect(width - 132.0, 11.0, 28.0, 28.0));
        self.scale_readout
            .setFrame(rect(width - 98.0, 14.0, 56.0, 22.0));
        self.scale_up.setFrame(rect(width - 38.0, 11.0, 28.0, 28.0));
        y += 58.0;
        self.bubble_visibility_card
            .setFrame(rect(8.0, y, width, 206.0));
        place_switch(
            &self.bubble_visible_label,
            &self.bubble_visible_switch,
            width,
            8.0,
        );
        self.bubble_placement_label
            .setFrame(rect(14.0, 44.0, content, 20.0));
        let button_width = ((content - 12.0) / 3.0).max(1.0);
        for (index, button) in self.placement_buttons.iter().enumerate() {
            button.setFrame(rect(
                14.0 + (index % 3) as f64 * (button_width + 6.0),
                68.0 + (index / 3) as f64 * 30.0,
                button_width,
                26.0,
            ));
        }
        self.bubble_theme_label
            .setFrame(rect(14.0, 134.0, content, 20.0));
        self.bubble_theme_popup
            .setFrame(rect(14.0, 158.0, content, 28.0));
        y += 214.0;
        self.detail_openers[0].setFrame(rect(8.0, y, width, 28.0));
        y += 36.0;
        self.reset.setFrame(rect(8.0, y, width, 28.0));
        y + 36.0
    }

    fn layout_connections_basic(&self, width: f64) -> f64 {
        let content = (width - 28.0).max(1.0);
        let mut inner = 12.0;
        for field in [
            &self.observation_title,
            &self.connection_selection_summary,
            &self.connection_summary,
        ] {
            place_observation_label(field, 14.0, content, 20.0, &mut inner, true);
            inner += 8.0;
        }
        if self.connection_has_error() {
            place_observation_label(
                &self.connection_error_summary,
                14.0,
                content,
                18.0,
                &mut inner,
                true,
            );
            inner += 8.0;
        }
        self.connection_error_summary
            .setHidden(!self.connection_has_error());
        self.observation_card
            .setFrame(rect(8.0, 8.0, width, inner + 4.0));
        let mut y = inner + 20.0;
        for index in [1, 4] {
            self.detail_openers[index].setFrame(rect(8.0, y, width, 28.0));
            y += 36.0;
        }
        y + 8.0
    }

    fn layout_app_basic(&self, width: f64) -> f64 {
        self.app_general_card.setFrame(rect(8.0, 8.0, width, 72.0));
        self.language_label
            .setFrame(rect(14.0, 8.0, (width - 28.0).max(1.0), 20.0));
        self.language_popup
            .setFrame(rect(14.0, 32.0, (width - 28.0).max(1.0), 28.0));
        self.detail_openers[2].setFrame(rect(8.0, 88.0, width, 28.0));
        let mut y = 124.0;
        let height = self.lifecycle_card.height(width);
        self.lifecycle_card.layout(rect(8.0, y, width, height));
        y += height + 8.0;
        let height = self.update_card.height(width);
        self.update_card.layout(rect(8.0, y, width, height));
        y += height + 8.0;
        let link_width = ((width - 8.0) * 0.5).max(1.0);
        for (column, index) in [3, 5].into_iter().enumerate() {
            self.detail_openers[index].setFrame(rect(
                8.0 + column as f64 * (link_width + 8.0),
                y,
                link_width,
                28.0,
            ));
        }
        y += 36.0;
        self.detail_openers[SettingsDetailPage::AppRecovery.index()]
            .setFrame(rect(8.0, y, width, 28.0));
        y += 36.0;
        y + 8.0
    }

    fn restore_detail_offset(&self, page: SettingsDetailPage) {
        let maximum = (self.detail_document.frame().size.height
            - self.detail_scroll.contentView().bounds().size.height)
            .max(0.0);
        self.detail_scroll.contentView().scrollToPoint(NSPoint::new(
            0.0,
            self.detail_offsets.get()[page.index()].clamp(0.0, maximum),
        ));
        self.detail_scroll
            .reflectScrolledClipView(&self.detail_scroll.contentView());
    }

    fn remember_detail_size(&self) {
        let size = self.detail_root.bounds().size;
        if let (Some(page), Some(last)) = (self.selected_detail, self.detail_layout_size.get()) {
            if self.detail_window.isVisible()
                && ((size.width - last.width).abs() > 0.5
                    || (size.height - last.height).abs() > 0.5)
            {
                let mut sizes = self.detail_user_sizes.get();
                sizes[page.index()] = Some(size);
                self.detail_user_sizes.set(sizes);
            }
        }
        self.detail_layout_size.set(Some(size));
    }

    fn layout_detail(&self) {
        let Some(page) = self.selected_detail else {
            return;
        };
        // Native marked text owns its geometry until composition ends. Keep the
        // dirty bit so the normal refresh applies the deferred fit afterward.
        if self.bubble_colors_marked() {
            self.detail_root.ivars().layout_dirty.set(true);
            return;
        }
        self.remember_detail_size();
        let old_offset = self.detail_scroll.contentView().bounds().origin.y;
        let anchor =
            scroll_reading_anchor(&self.detail_document, &self.detail_document, old_offset);
        let frame = self.detail_window.frame();
        let visible = self
            .detail_window
            .screen()
            .or_else(|| NSScreen::mainScreen(self.detail_window.mtm()))
            .map(|screen| screen.visibleFrame())
            .unwrap_or(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(800.0, 700.0),
            ));
        let decoration = (frame.size.height - self.detail_root.bounds().size.height).max(0.0);
        let maximum = NSSize::new(
            (visible.size.width - 32.0).max(1.0),
            (visible.size.height - decoration - 32.0).max(1.0),
        );
        let minimum = NSSize::new(
            DETAIL_MIN_WIDTH.min(maximum.width),
            DETAIL_MIN_HEIGHT.min(maximum.height),
        );
        let preferred = self.detail_user_sizes.get()[page.index()];
        let width = preferred
            .map_or(DETAIL_DEFAULT_WIDTH, |size| size.width)
            .clamp(minimum.width, maximum.width);
        let content_width = (width - 30.0).max(1.0);
        let measured_height = self.layout_detail_content(page, content_width);
        let height = preferred
            .map_or(measured_height.min(DETAIL_DEFAULT_MAX_HEIGHT), |size| {
                size.height
            })
            .clamp(minimum.height, maximum.height);
        self.detail_window.setContentMinSize(minimum);
        self.detail_window.setContentMaxSize(maximum);
        let size = NSSize::new(width, height);
        let current = self.detail_root.bounds().size;
        if (current.width - width).abs() > 0.5 || (current.height - height).abs() > 0.5 {
            self.detail_window.setContentSize(size);
            // AppKit's content-size adjustment must not move the reading edge.
            // Clamp only when the retained top edge would leave this display.
            let resized = self.detail_window.frame().size;
            self.detail_window.setFrameOrigin(NSPoint::new(
                frame.origin.x.clamp(
                    visible.origin.x,
                    (visible.origin.x + visible.size.width - resized.width).max(visible.origin.x),
                ),
                (frame.origin.y + frame.size.height - resized.height).clamp(
                    visible.origin.y,
                    (visible.origin.y + visible.size.height - resized.height).max(visible.origin.y),
                ),
            ));
        }
        self.detail_root.setFrame(rect(0.0, 0.0, width, height));
        self.detail_scroll.setFrame(self.detail_root.bounds());
        let document_width = self
            .detail_scroll
            .contentView()
            .bounds()
            .size
            .width
            .max(1.0);
        let document_height = measured_height.max(height);
        self.detail_documents[page.index()].setFrame(rect(
            0.0,
            0.0,
            document_width,
            document_height,
        ));
        self.detail_document
            .setFrame(rect(0.0, 0.0, document_width, document_height));
        let maximum_offset = (document_height - height).max(0.0);
        let offset = anchor
            .filter(|(view, _)| {
                let hidden: bool = unsafe { msg_send![&**view, isHiddenOrHasHiddenAncestor] };
                view.window().is_some() && !hidden
            })
            .map_or(old_offset, |(view, old_y)| {
                old_offset
                    + view
                        .convertRect_toView(view.bounds(), Some(&self.detail_document))
                        .origin
                        .y
                    - old_y
            })
            .clamp(0.0, maximum_offset);
        self.detail_scroll
            .contentView()
            .scrollToPoint(NSPoint::new(0.0, offset));
        self.detail_scroll
            .reflectScrolledClipView(&self.detail_scroll.contentView());
        self.detail_window.recalculateKeyViewLoop();
        self.detail_layout_size.set(Some(size));
        self.detail_root.ivars().layout_dirty.set(false);
    }

    fn layout_detail_content(&self, page: SettingsDetailPage, width: f64) -> f64 {
        match page {
            SettingsDetailPage::Appearance => self.layout_appearance_detail(width),
            SettingsDetailPage::Connections => self.layout_connections_detail(width),
            SettingsDetailPage::MenuBar => self.layout_menu_bar_detail(width),
            SettingsDetailPage::UpdateSettings => {
                let height = self.update_card.settings_height(width);
                self.update_card
                    .layout_settings(rect(8.0, 8.0, width, height));
                height + 16.0
            }
            SettingsDetailPage::ConnectionDiagnostics => self.layout_connection_diagnostics(width),
            SettingsDetailPage::UpdateDetails => {
                let height = self.update_card.details_height(width);
                self.update_card
                    .layout_details(rect(8.0, 8.0, width, height));
                height + 16.0
            }
            SettingsDetailPage::AppRecovery => self.layout_recovery_detail(width),
        }
    }

    fn layout_recovery_detail(&self, width: f64) -> f64 {
        let content = (width - 28.0).max(1.0);
        let mut y = 16.0;
        for field in [&self.recovery_reset_help, &self.recovery_preserved_help] {
            place_observation_label(field, 22.0, content, 18.0, &mut y, true);
            y += 12.0;
        }
        if !self.settings_recovery_status.isHidden() {
            let height =
                measured_label_height(&self.settings_recovery_status, content).clamp(18.0, 144.0);
            self.settings_recovery_status
                .setFrame(rect(22.0, y, content, height));
            y += height + 12.0;
        }
        self.reset_settings.setFrame(rect(22.0, y, content, 30.0));
        y + 46.0
    }

    fn layout_appearance_detail(&self, width: f64) -> f64 {
        let content = (width - 28.0).max(1.0);
        self.status_indicators_label
            .setFrame(rect(22.0, 16.0, (width - 80.0).max(1.0), 24.0));
        self.status_indicators_switch
            .setFrame(rect(width - 50.0, 12.0, 46.0, 30.0));
        let mut inner = 12.0;
        for field in [&self.settings_appearance_label, &self.click_behavior_label] {
            place_observation_label(field, 14.0, content, 20.0, &mut inner, true);
            inner += 6.0;
        }
        for (name, toggle, help) in [
            (
                &self.full_passthrough_label,
                &self.full_passthrough_switch,
                &self.full_passthrough_help,
            ),
            (
                &self.alpha_passthrough_label,
                &self.alpha_passthrough_switch,
                &self.alpha_passthrough_help,
            ),
        ] {
            place_switch(name, toggle, width, inner);
            inner += 34.0;
            place_observation_label(help, 14.0, content, 16.0, &mut inner, true);
            inner += 10.0;
        }
        self.appearance_card.setFrame(rect(8.0, 50.0, width, inner));
        let mut y = 50.0 + inner + 8.0;
        let mut color_y = 12.0;
        place_observation_label(
            &self.bubble_colors_label,
            14.0,
            content,
            18.0,
            &mut color_y,
            true,
        );
        color_y += 10.0;
        for ((name, field), swatch) in self
            .bubble_color_labels
            .iter()
            .zip(&self.bubble_color_fields)
            .zip(&self.bubble_color_swatches)
        {
            name.setFrame(rect(14.0, color_y + 2.0, (width - 154.0).max(1.0), 20.0));
            swatch.setFrame(rect(width - 126.0, color_y + 2.0, 18.0, 18.0));
            field.setFrame(rect(width - 100.0, color_y, 86.0, 22.0));
            color_y += 30.0;
        }
        let half = ((content - 8.0) / 2.0).max(1.0);
        self.bubble_apply.setFrame(rect(14.0, color_y, half, 28.0));
        self.bubble_reset
            .setFrame(rect(22.0 + half, color_y, half, 28.0));
        color_y += 36.0;
        for button in [&self.bubble_reload, &self.bubble_rebase] {
            button.setHidden(!self.bubble_draft.conflicted());
            if self.bubble_draft.conflicted() {
                button.setFrame(rect(14.0, color_y, content, 28.0));
                color_y += 34.0;
            }
        }
        self.bubble_theme_card
            .setFrame(rect(8.0, y, width, color_y + 8.0));
        y += color_y + 16.0;
        self.character_editing_open
            .setFrame(rect(8.0, y, width, 30.0));
        y += 40.0;
        place_observation_label(&self.appearance_error, 22.0, content, 18.0, &mut y, true);
        y + 16.0
    }

    fn layout_connections_detail(&self, width: f64) -> f64 {
        let content = (width - 28.0).max(1.0);
        let mut y = 16.0;
        place_observation_label(
            &self.connection_host_label,
            22.0,
            content,
            20.0,
            &mut y,
            true,
        );
        y += 6.0;
        self.connection_host_popup
            .setFrame(rect(22.0, y, content, 28.0));
        y += 38.0;
        let auto = self
            .observation_preferences
            .as_ref()
            .is_none_or(|p| p.host == ObservationSelection::Auto);
        self.connection_auto_help.setHidden(!auto);
        if auto {
            place_observation_label(
                &self.connection_auto_help,
                22.0,
                content,
                16.0,
                &mut y,
                true,
            );
            y += 12.0;
        }
        place_observation_label(&self.connection_status, 22.0, content, 18.0, &mut y, true);
        y += 12.0;
        let height = self.lifecycle_card.extensions_height(width);
        self.lifecycle_card
            .layout_extensions(rect(8.0, y, width, height));
        y += height + 12.0;
        self.connection_settings_error
            .setHidden(!self.connection_has_error());
        if self.connection_has_error() {
            place_observation_label(
                &self.connection_settings_error,
                22.0,
                content,
                18.0,
                &mut y,
                true,
            );
        }
        y + 16.0
    }

    fn layout_connection_diagnostics(&self, width: f64) -> f64 {
        let content = (width - 28.0).max(1.0);
        let mut y = 16.0;
        place_observation_label(
            &self.connection_profile_title,
            22.0,
            content,
            20.0,
            &mut y,
            true,
        );
        y += 12.0;
        for (title, value) in self
            .connection_profile_labels
            .iter()
            .zip(&self.connection_profile_values)
        {
            place_observation_label(title, 22.0, content, 16.0, &mut y, true);
            y += 4.0;
            place_observation_label(value, 22.0, content, 18.0, &mut y, true);
            y += 12.0;
        }
        for field in [&self.observation_error, &self.lifecycle_error_detail] {
            field.setHidden(false);
            place_observation_label(field, 22.0, content, 18.0, &mut y, true);
            y += 12.0;
        }
        y + 8.0
    }

    fn layout_menu_bar_detail(&self, width: f64) -> f64 {
        let content = (width - 28.0).max(1.0);
        let mut y = 12.0;
        for field in [&self.menu_bar_label, &self.menu_bar_visibility] {
            place_observation_label(field, 14.0, content, 20.0, &mut y, true);
            y += 6.0;
        }
        self.menu_bar_popup.setFrame(rect(14.0, y, content, 28.0));
        y += 38.0;
        place_observation_label(&self.menu_bar_help, 14.0, content, 18.0, &mut y, true);
        y += 12.0;
        self.menu_bar_separator
            .setFrame(rect(14.0, y, content, 1.0));
        self.menu_bar_preview_well
            .setFrame(rect(14.0, y + 10.0, 36.0, 36.0));
        self.menu_bar_preview.setFrame(rect(9.0, 9.0, 18.0, 18.0));
        self.menu_bar_icon_status
            .setFrame(rect(60.0, y + 18.0, (content - 46.0).max(1.0), 22.0));
        y += 58.0;
        for button in [&self.menu_bar_choose, &self.menu_bar_restore] {
            button.setFrame(rect(14.0, y, content, 28.0));
            y += 36.0;
        }
        place_observation_label(&self.menu_bar_icon_help, 14.0, content, 18.0, &mut y, true);
        if self.menu_bar_icon_error_detail.is_some() {
            y += 12.0;
            place_observation_label(&self.menu_bar_icon_error, 14.0, content, 18.0, &mut y, true);
        }
        self.menu_bar_card.setFrame(rect(8.0, 8.0, width, y + 12.0));
        y + 28.0
    }

    fn localize_detail_openers(&self) {
        for (index, button) in self.detail_openers.iter().enumerate() {
            let page = SettingsDetailPage::from_tag(index as isize).expect("known detail page");
            let error = match page {
                SettingsDetailPage::Appearance => {
                    self.presentation_error.is_some() || self.bubble_draft.conflicted()
                }
                SettingsDetailPage::Connections | SettingsDetailPage::ConnectionDiagnostics => {
                    self.connection_has_error() || self.lifecycle_card.error_detail().is_some()
                }
                SettingsDetailPage::MenuBar => self.menu_bar_icon_error_detail.is_some(),
                SettingsDetailPage::UpdateDetails => self.update_details_problem,
                _ => false,
            };
            let title = format!(
                "{}{}…",
                if error { "⚠ " } else { "" },
                text(self.locale, page.message())
            );
            button.setTitle(&NSString::from_str(&title));
            set_accessibility_label(button, &title);
        }
        self.footer_details.setTitle(&NSString::from_str(text(
            self.locale,
            Message::SettingsDetailsLink,
        )));
        set_accessibility_label(
            &self.footer_details,
            &format!(
                "{} · {}",
                text(self.locale, Message::AppearanceDetails),
                text(self.locale, Message::SettingsDetailsLink)
            ),
        );
        if let Some(page) = self.selected_detail {
            self.detail_window
                .setTitle(&NSString::from_str(text(self.locale, page.message())));
        }
    }
}

fn scroll_reading_anchor(
    view: &NSView,
    document: &NSView,
    offset: f64,
) -> Option<(Retained<NSView>, f64)> {
    let mut best: Option<(Retained<NSView>, f64)> = None;
    for child in view.subviews().iter() {
        if child.isHidden() {
            continue;
        }
        let candidate = if child.downcast_ref::<NSControl>().is_some() {
            let y = child
                .convertRect_toView(child.bounds(), Some(document))
                .origin
                .y;
            Some((child.retain(), y))
        } else {
            scroll_reading_anchor(&child, document, offset)
        };
        if let Some((candidate, y)) = candidate {
            if best
                .as_ref()
                .is_none_or(|(_, best_y)| (y - offset).abs() < (*best_y - offset).abs())
            {
                best = Some((candidate, y));
            }
        }
    }
    best
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(x, y),
        NSSize::new(width.max(1.0), height.max(1.0)),
    )
}

fn place_switch(name: &NSTextField, toggle: &NSSwitch, width: f64, y: f64) {
    name.setFrame(rect(14.0, y + 3.0, (width - 80.0).max(1.0), 24.0));
    toggle.setFrame(rect(width - 58.0, y, 46.0, 30.0));
}

fn diagnostic_label(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = label("", 11.0, false, secondary(), mtm);
    field.setMaximumNumberOfLines(0);
    field.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByCharWrapping);
    field.setEditable(false);
    field.setSelectable(true);
    field.setRefusesFirstResponder(false);
    field
}

fn set_readonly_value(field: &NSTextField, value: &str) {
    if field.stringValue().to_string() != value {
        field.setStringValue(&NSString::from_str(value));
        set_accessibility_label(field, value);
    }
}

fn connection_selected_count(
    selection: &ObservationSelection,
    providers: &std::collections::BTreeMap<String, String>,
    connections: &HostConnections,
) -> usize {
    providers
        .keys()
        .filter(|provider| selection.includes_provider(provider))
        .map(|provider| connections.get(provider).copied().unwrap_or_default())
        .sum()
}

fn connection_query_message(unavailable: bool, has_error: bool) -> Option<Message> {
    if unavailable {
        Some(Message::SettingsSnapshotUnavailable)
    } else if has_error {
        Some(Message::SettingsConnectionError)
    } else {
        None
    }
}

fn connection_summary_message(
    selection: &ObservationSelection,
    providers: &std::collections::BTreeMap<String, String>,
    connections: &HostConnections,
) -> Message {
    if matches!(selection, ObservationSelection::Provider(provider) if !providers.contains_key(provider))
    {
        Message::ConnectionUnavailable
    } else if providers.is_empty() {
        Message::ConnectionNoProviders
    } else if connection_selected_count(selection, providers, connections) > 0 {
        Message::ConnectionConnected
    } else {
        Message::ConnectionWaiting
    }
}

fn measured_label_height(field: &NSTextField, width: f64) -> f64 {
    // NSCell measures the actual localized glyphs and wraps at the available width.
    let size = field
        .cell()
        .expect("wrapping label cell")
        .cellSizeForBounds(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width.max(1.0), 100_000.0),
        ));
    size.height.ceil()
}

fn place_observation_label(
    field: &NSTextField,
    x: f64,
    width: f64,
    minimum: f64,
    y: &mut f64,
    apply: bool,
) {
    let height = measured_label_height(field, width).max(minimum);
    if apply {
        field.setFrame(NSRect::new(
            NSPoint::new(x, *y),
            NSSize::new(width.max(1.0), height),
        ));
    }
    *y += height;
}

fn configure_panel(panel: &NSPanel) {
    // SAFETY: the panel is not owned by a window controller and therefore
    // must not release itself when closed.
    unsafe { panel.setReleasedWhenClosed(false) };
    // A borderless NSPanel needs an explicit window accessibility identity;
    // the unexposed content view still passes through its native child controls.
    unsafe {
        let _: () = msg_send![panel, setAccessibilityElement: true];
        let _: () = msg_send![
            panel,
            setAccessibilityRole: Some(objc2_app_kit::NSAccessibilityWindowRole)
        ];
        let _: () = msg_send![
            panel,
            setAccessibilitySubrole: Some(objc2_app_kit::NSAccessibilityStandardWindowSubrole)
        ];
    }
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(false);
    panel.setWorksWhenModal(true);
    panel.setLevel(NSFloatingWindowLevel);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::MoveToActiveSpace
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setOpaque(false);
    let clear = NSColor::clearColor();
    panel.setBackgroundColor(Some(&clear));
    panel.setHasShadow(true);
    // Keep the menu ordered across transient focus changes; the app delegate
    // and explicit event monitors own dismissal.
    panel.setHidesOnDeactivate(false);
    panel.setAcceptsMouseMovedEvents(true);
}

fn make_tab(
    symbol: &str,
    title: &str,
    index: usize,
    target: &MenuTarget,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
    };
    button.setButtonType(NSButtonType::Toggle);
    button.setBordered(false);
    button.setEnabled(true);
    button.setRefusesFirstResponder(false);
    button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    button.setImageHugsTitle(true);
    button.setImagePosition(NSCellImagePosition::ImageLeading);
    button.setImageScaling(NSImageScaling::ScaleProportionallyDown);
    let symbol_name = NSString::from_str(symbol);
    if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &symbol_name,
        Some(&NSString::from_str(title)),
    ) {
        button.setImage(Some(&image));
    }
    unsafe {
        let _: () = msg_send![&*button, setTag: index as isize];
    }
    bind_control(&button, target, sel!(selectMenuTab:));
    set_accessibility_label(&*button, title);
    // SAFETY: AppKit exports this immutable NSString constant for the process lifetime.
    set_accessibility_role(&*button, unsafe {
        objc2_app_kit::NSAccessibilityRadioButtonRole
    });
    // SAFETY: AppKit exports this immutable NSString constant for the process lifetime.
    set_accessibility_subrole(&*button, unsafe {
        objc2_app_kit::NSAccessibilityTabButtonSubrole
    });
    button
}
fn make_switch(
    target: &MenuTarget,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSSwitch> {
    let control: Retained<NSSwitch> = unsafe {
        msg_send![
            NSSwitch::alloc(mtm),
            initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(46.0, 30.0))
        ]
    };
    control.setControlSize(NSControlSize::Small);
    control.setEnabled(true);
    control.setRefusesFirstResponder(false);
    bind_control(&control, target, action);
    control
}

fn make_segment_button(
    title: &str,
    target: &MenuTarget,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
    };
    button.setButtonType(NSButtonType::PushOnPushOff);
    button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    button.setContentTintColor(Some(&primary()));
    bind_control(&button, target, action);
    set_accessibility_label(&*button, title);
    button
}

fn make_selection_popup(
    items: &[(Message, isize)],
    locale: UiLocale,
    target: &MenuTarget,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSPopUpButton> {
    let popup: Retained<NSPopUpButton> = unsafe {
        msg_send![
            NSPopUpButton::alloc(mtm),
            initWithFrame: NSRect::default(),
            pullsDown: false
        ]
    };
    popup.setControlSize(NSControlSize::Small);
    popup.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    popup.setContentTintColor(Some(&primary()));
    popup.setRefusesFirstResponder(false);
    for &(message, tag) in items {
        popup.addItemWithTitle(&NSString::from_str(text(locale, message)));
        popup.lastItem().expect("new popup item").setTag(tag);
    }
    bind_control(&popup, target, action);
    popup
}

fn localize_popup_items(popup: &NSPopUpButton, locale: UiLocale, items: &[(Message, isize)]) {
    let menu = popup.menu().expect("selection popup menu");
    for &(message, tag) in items {
        menu.itemWithTag(tag)
            .expect("selection popup item")
            .setTitle(&NSString::from_str(text(locale, message)));
    }
}
fn make_action_button(
    title: &str,
    target: &MenuTarget,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
    };
    button.setButtonType(NSButtonType::MomentaryPushIn);
    button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    button.setBordered(true);
    button.setEnabled(true);
    button.setRefusesFirstResponder(false);
    button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    button.setContentTintColor(Some(&primary()));
    bind_control(&button, target, action);
    button
}

fn make_stepper_button(
    title: &str,
    target: &MenuTarget,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
    };
    button.setButtonType(NSButtonType::MomentaryPushIn);
    button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    button.setBordered(true);
    button.setEnabled(true);
    button.setRefusesFirstResponder(false);
    button.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    button.setContentTintColor(Some(&primary()));
    bind_control(&button, target, action);
    button
}

fn make_tab_indicator(mtm: MainThreadMarker) -> Retained<NSBox> {
    let indicator = NSBox::initWithFrame(
        NSBox::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 26.0)),
    );
    indicator.setBoxType(NSBoxType::Custom);
    indicator.setTransparent(false);
    indicator.setBorderWidth(1.0);
    indicator.setCornerRadius(6.0);
    let fill = color(CARD_RED, CARD_GREEN, CARD_BLUE, 0.98);
    let border = color(CARD_BORDER_RED, CARD_BORDER_GREEN, CARD_BORDER_BLUE, 0.65);
    indicator.setFillColor(&fill);
    indicator.setBorderColor(&border);
    unsafe {
        let _: () = msg_send![&*indicator, setAccessibilityElement: false];
    }
    indicator
}

fn make_color_swatch(mtm: MainThreadMarker) -> Retained<NSBox> {
    let swatch = NSBox::initWithFrame(
        NSBox::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(18.0, 18.0)),
    );
    swatch.setBoxType(NSBoxType::Custom);
    swatch.setTransparent(false);
    swatch.setBorderWidth(1.0);
    swatch.setCornerRadius(4.0);
    let border = color(CARD_BORDER_RED, CARD_BORDER_GREEN, CARD_BORDER_BLUE, 0.55);
    swatch.setBorderColor(&border);
    let fill = color(CARD_RED, CARD_GREEN, CARD_BLUE, 1.0);
    swatch.setFillColor(&fill);
    unsafe {
        let _: () = msg_send![&*swatch, setAccessibilityElement: false];
    }
    swatch
}

fn bind_control(control: &NSControl, target: &MenuTarget, action: objc2::runtime::Sel) {
    unsafe {
        let _: () = msg_send![control, setTarget: Some(target)];
        let _: () = msg_send![control, setAction: Some(action)];
    }
}

fn set_action(control: &NSControl, target: &MenuTarget, action: objc2::runtime::Sel) {
    bind_control(control, target, action);
}

fn label(
    value: &str,
    size: f64,
    bold: bool,
    foreground: Retained<NSColor>,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&NSString::from_str(value), mtm);
    field.setAlignment(NSTextAlignment::Left);
    let font = if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    field.setTextColor(Some(&foreground));
    field.setDrawsBackground(false);
    field.setBordered(false);
    field.setBezeled(false);
    field.setEditable(false);
    field.setSelectable(false);
    field.setUsesSingleLineMode(false);
    field.setMaximumNumberOfLines(2);
    field.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByWordWrapping);
    field
}

fn set_accessibility_label(view: &NSView, value: &str) {
    let value = NSString::from_str(value);
    unsafe {
        let _: () = msg_send![view, setAccessibilityLabel: Some(&*value)];
    }
}

fn set_accessibility_identifier(view: &NSView, value: &str) {
    let value = NSString::from_str(value);
    unsafe {
        let _: () = msg_send![view, setAccessibilityIdentifier: Some(&*value)];
    }
}

fn set_tooltip(view: &NSView, value: &str) {
    let value = NSString::from_str(value);
    unsafe {
        let _: () = msg_send![view, setToolTip: Some(&*value)];
    }
}

fn set_accessibility_element(view: &NSView, enabled: bool) {
    unsafe {
        let _: () = msg_send![view, setAccessibilityElement: enabled];
    }
}

fn set_accessibility_selected(view: &NSView, selected: bool) {
    unsafe {
        let _: () = msg_send![view, setAccessibilitySelected: selected];
    }
}

fn set_accessibility_role(view: &NSView, role: &NSString) {
    unsafe {
        let _: () = msg_send![view, setAccessibilityRole: Some(role)];
    }
}

fn set_accessibility_subrole(view: &NSView, subrole: &NSString) {
    unsafe {
        let _: () = msg_send![view, setAccessibilitySubrole: Some(subrole)];
    }
}

fn color(red: f64, green: f64, blue: f64, alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha)
}

fn primary() -> Retained<NSColor> {
    color(PRIMARY_RED, PRIMARY_GREEN, PRIMARY_BLUE, 1.0)
}

fn secondary() -> Retained<NSColor> {
    color(SECONDARY_RED, SECONDARY_GREEN, SECONDARY_BLUE, 1.0)
}

fn inset_rect(rect: NSRect, inset: f64) -> NSRect {
    let inset = inset.max(0.0);
    NSRect::new(
        NSPoint::new(rect.origin.x + inset, rect.origin.y + inset),
        NSSize::new(
            (rect.size.width - inset * 2.0).max(0.0),
            (rect.size.height - inset * 2.0).max(0.0),
        ),
    )
}

fn placement_index(placement: BubblePlacement) -> usize {
    match placement {
        BubblePlacement::Auto => 0,
        BubblePlacement::Above => 1,
        BubblePlacement::Below => 2,
        BubblePlacement::Left => 3,
        BubblePlacement::Right => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_appearance_preserves_editing_baseline_until_explicit_rebase() {
        let original = BubbleAppearance::default();
        let external = BubbleAppearance {
            theme: BubbleTheme::DustyRose,
            ..original
        };
        let mut draft = BubbleColorDraft::default();
        draft.receive(original, false);
        draft.receive(external, true);
        assert_eq!(draft.baseline, Some(original));
        assert_eq!(draft.latest, Some(external));
        assert!(draft.conflicted());
        draft.receive(external, false);
        assert!(draft.conflicted());
        assert_eq!(draft.baseline, Some(original));
        assert!(draft.rebase());
        assert_eq!(draft.baseline, Some(external));
        assert!(!draft.conflicted());
    }

    #[test]
    fn clean_external_update_and_own_save_advance_color_baseline() {
        let original = BubbleAppearance::default();
        let external = BubbleAppearance {
            theme: BubbleTheme::MoonlitInk,
            ..original
        };
        let custom = BubbleAppearance {
            theme: BubbleTheme::Custom,
            ..original
        };
        let mut draft = BubbleColorDraft::default();
        draft.receive(original, false);
        draft.receive(external, false);
        assert_eq!(draft.baseline, Some(external));
        assert!(!draft.conflicted());
        draft.receive(custom, true);
        assert!(draft.conflicted());
        draft.saved(custom);
        assert_eq!(draft.baseline, Some(custom));
        assert!(!draft.conflicted());
    }

    #[test]
    fn connection_summary_only_counts_registered_selected_providers() {
        let providers = [
            ("tern".into(), "Tern".into()),
            ("other".into(), "Other".into()),
        ]
        .into();
        let connections = [("tern".into(), 2), ("removed".into(), 7)].into();
        assert_eq!(
            connection_selected_count(&ObservationSelection::Auto, &providers, &connections),
            2
        );
        assert_eq!(
            connection_summary_message(&ObservationSelection::Auto, &providers, &connections),
            Message::ConnectionConnected
        );
        assert_eq!(
            connection_summary_message(
                &ObservationSelection::Provider("other".into()),
                &providers,
                &connections
            ),
            Message::ConnectionWaiting
        );
        assert_eq!(
            connection_selected_count(
                &ObservationSelection::Provider("other".into()),
                &providers,
                &connections
            ),
            0
        );
        assert_eq!(
            connection_summary_message(
                &ObservationSelection::Provider("removed".into()),
                &providers,
                &connections
            ),
            Message::ConnectionUnavailable
        );
        assert_eq!(
            connection_summary_message(
                &ObservationSelection::Auto,
                &Default::default(),
                &connections
            ),
            Message::ConnectionNoProviders
        );
        assert_eq!(
            connection_summary_message(
                &ObservationSelection::Provider("missing".into()),
                &Default::default(),
                &connections
            ),
            Message::ConnectionUnavailable
        );
        let stale = [("removed".into(), 7)].into();
        assert_eq!(
            connection_summary_message(&ObservationSelection::Auto, &providers, &stale),
            Message::ConnectionWaiting
        );
    }

    #[test]
    fn unavailable_snapshot_is_distinct_from_empty_and_does_not_replace_connection_state() {
        let providers = [("tern".into(), "Tern".into())].into();
        let connections = [("tern".into(), 2)].into();
        assert_eq!(connection_query_message(false, false), None);
        assert_eq!(
            connection_query_message(true, false),
            Some(Message::SettingsSnapshotUnavailable)
        );
        assert_eq!(
            connection_query_message(true, true),
            Some(Message::SettingsSnapshotUnavailable)
        );
        assert_eq!(
            connection_query_message(false, true),
            Some(Message::SettingsConnectionError)
        );
        assert_eq!(
            connection_summary_message(&ObservationSelection::Auto, &providers, &connections),
            Message::ConnectionConnected
        );
        assert_eq!(
            connection_summary_message(
                &ObservationSelection::Auto,
                &Default::default(),
                &Default::default()
            ),
            Message::ConnectionNoProviders
        );
    }
}
