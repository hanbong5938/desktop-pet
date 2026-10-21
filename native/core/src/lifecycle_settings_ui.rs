use crate::i18n::{text, Message, UiLocale};
use crate::lifecycle::{LifecycleSetting, LifecycleSettings};
use desktop_pet_contracts::{LifecycleActionView, LifecycleSettingView};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    define_class, msg_send, sel, MainThreadMarker, MainThreadOnly, Message as ObjcMessage,
};
use objc2_app_kit::{
    NSBezierPath, NSButton, NSColor, NSControlSize, NSControlStateValueOff, NSControlStateValueOn,
    NSFont, NSSwitch, NSTextField, NSUserInterfaceItemIdentification, NSView,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

// SAFETY: This decorative AppKit view is constructed and used on the main thread.
define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetLifecycleCardView"]
    struct CardView;
    unsafe impl NSObjectProtocol for CardView {}
    impl CardView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _rect: NSRect) {
            let bounds = self.bounds();
            NSColor::colorWithSRGBRed_green_blue_alpha(0.161, 0.161, 0.176, 1.0).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, 10.0, 10.0).fill();
            NSColor::colorWithSRGBRed_green_blue_alpha(0.28, 0.28, 0.32, 0.5).setStroke();
            let border = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                NSRect::new(NSPoint::new(0.5, 0.5), NSSize::new((bounds.size.width - 1.0).max(1.0), (bounds.size.height - 1.0).max(1.0))),
                9.5, 9.5,
            );
            border.setLineWidth(1.0);
            border.stroke();
        }
    }
);

impl CardView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), initWithFrame: NSRect::default()] }
    }
}

fn label(
    value: &str,
    size: f64,
    bold: bool,
    muted: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&NSString::from_str(value), mtm);
    let font = if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    let color = if muted {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.60, 0.61, 0.66, 1.0)
    } else {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.95, 0.95, 0.97, 1.0)
    };
    field.setTextColor(Some(&color));
    field.setMaximumNumberOfLines(0);
    field
}

fn accessibility(view: &NSView, value: &str) {
    let label = NSString::from_str(value);
    unsafe {
        let _: () = msg_send![view, setAccessibilityLabel: Some(&*label)];
    }
}

pub(crate) struct LifecycleSettingsCard {
    view: Retained<CardView>,
    extensions_view: Retained<CardView>,
    extensions_empty: Retained<NSTextField>,
    heading: Retained<NSTextField>,
    auto_label: Retained<NSTextField>,
    error_details: Retained<NSButton>,
    auto_switch: Retained<NSSwitch>,
    error: Retained<NSTextField>,
    locale: UiLocale,
    error_detail: Option<String>,
    target: Retained<AnyObject>,
    mtm: MainThreadMarker,
    extension_rows: Vec<SettingRow>,
    action_rows: Vec<ActionRow>,
    settings_snapshot: Vec<(String, LifecycleSettingView)>,
    actions_snapshot: Vec<(String, LifecycleActionView)>,
    extensions_available: bool,
}

struct SettingRow {
    provider: String,
    key: String,
    label: Retained<NSTextField>,
    help: Retained<NSTextField>,
    toggle: Retained<NSSwitch>,
}

struct ActionRow {
    provider: String,
    key: String,
    help: Retained<NSTextField>,
    button: Retained<NSButton>,
}

impl LifecycleSettingsCard {
    pub(crate) fn new(locale: UiLocale, target: &AnyObject, mtm: MainThreadMarker) -> Self {
        let view = CardView::new(mtm);
        let extensions_view = CardView::new(mtm);
        // The provider document is mounted directly in Connections by MenuPanel.
        let extensions_empty = label("", 10.5, false, true, mtm);
        extensions_view.addSubview(&extensions_empty);
        unsafe {
            let _: () = msg_send![&*view, setAccessibilityElement: false];
        }
        let heading = label("", 12.5, true, false, mtm);
        let auto_label = label("", 12.0, true, false, mtm);
        let error_details = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(""),
                Some(target),
                Some(sel!(openSettingsDetail:)),
                mtm,
            )
        };
        error_details.setTag(4);
        error_details.setRefusesFirstResponder(false);
        let error = label("", 10.5, false, false, mtm);
        error.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            1.0, 0.54, 0.50, 1.0,
        )));
        let make_switch = |action| {
            let switch: Retained<NSSwitch> = unsafe {
                msg_send![NSSwitch::alloc(mtm), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(46.0, 30.0))]
            };
            switch.setControlSize(NSControlSize::Small);
            switch.setRefusesFirstResponder(false);
            unsafe {
                switch.setTarget(Some(target));
                switch.setAction(Some(action));
            }
            switch
        };
        let auto_switch = make_switch(sel!(toggleLifecycleAutoStart:));
        for child in [
            &*heading as &NSView,
            &*auto_label,
            &*error_details,
            &*auto_switch,
            &*error,
        ] {
            view.addSubview(child);
        }
        let mut card = Self {
            view,
            extensions_view,
            extensions_empty,
            heading,
            auto_label,
            error_details,
            auto_switch,
            error,
            locale,
            error_detail: None,
            target: target.retain(),
            mtm,
            extension_rows: Vec::new(),
            action_rows: Vec::new(),
            settings_snapshot: Vec::new(),
            actions_snapshot: Vec::new(),
            extensions_available: true,
        };
        card.set_locale(locale);
        card
    }

    pub(crate) fn view(&self) -> &NSView {
        &self.view
    }

    pub(crate) fn height(&self, width: f64) -> f64 {
        self.layout_content(width, false)
    }

    pub(crate) fn layout(&self, frame: NSRect) {
        self.view.setFrame(frame);
        self.layout_content(frame.size.width, true);
    }

    fn layout_content(&self, width: f64, apply: bool) -> f64 {
        let text_width = (width - 82.0).max(1.0);
        let full_width = (width - 28.0).max(1.0);
        let mut y = 10.0;
        let place = |field: &NSTextField, width: f64, minimum: f64, y: &mut f64| {
            let height = field
                .cell()
                .expect("lifecycle label cell")
                .cellSizeForBounds(NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(width, 100_000.0),
                ))
                .height
                .ceil()
                .max(minimum);
            if apply {
                field.setFrame(NSRect::new(
                    NSPoint::new(14.0, *y),
                    NSSize::new(width, height),
                ));
            }
            *y += height;
        };
        place(&self.heading, full_width, 0.0, &mut y);
        y += 6.0;
        let auto_y = y;
        place(&self.auto_label, text_width, 32.0, &mut y);
        if apply {
            self.auto_switch.setFrame(NSRect::new(
                NSPoint::new(width - 58.0, auto_y),
                NSSize::new(46.0, 30.0),
            ));
        }
        y += 4.0;
        if !self.error.isHidden() {
            y += 6.0;
            place(&self.error, full_width, 0.0, &mut y);
            y += 4.0;
            if apply {
                self.error_details.setFrame(NSRect::new(
                    NSPoint::new(14.0, y),
                    NSSize::new(full_width, 26.0),
                ));
            }
            y += 26.0;
        }
        y + 10.0
    }

    pub(crate) fn separate_extensions(&mut self) -> &NSView {
        &self.extensions_view
    }

    pub(crate) fn extensions_height(&self, width: f64) -> f64 {
        self.layout_extensions_content(width, false)
    }

    pub(crate) fn layout_extensions(&self, frame: NSRect) {
        self.extensions_view.setFrame(frame);
        self.layout_extensions_content(frame.size.width, true);
    }

    fn layout_extensions_content(&self, width: f64, apply: bool) -> f64 {
        let full_width = (width - 28.0).max(1.0);
        let text_width = (width - 82.0).max(1.0);
        let mut y = 0.0;
        let place = |field: &NSTextField, width: f64, minimum: f64, y: &mut f64| {
            let height = field
                .cell()
                .expect("extension label cell")
                .cellSizeForBounds(NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(width, 100_000.0),
                ))
                .height
                .ceil()
                .max(minimum);
            if apply {
                field.setFrame(NSRect::new(
                    NSPoint::new(14.0, *y),
                    NSSize::new(width, height),
                ));
            }
            *y += height;
        };
        if self.extension_rows.is_empty() && self.action_rows.is_empty() {
            place(&self.extensions_empty, full_width, 14.0, &mut y);
            y += 6.0;
        }
        for row in &self.extension_rows {
            let row_y = y;
            place(&row.label, text_width, 32.0, &mut y);
            if apply {
                row.toggle.setFrame(NSRect::new(
                    NSPoint::new(width - 58.0, row_y),
                    NSSize::new(46.0, 30.0),
                ));
            }
            y += 2.0;
            place(&row.help, full_width, 0.0, &mut y);
            y += 6.0;
        }
        for row in &self.action_rows {
            place(&row.help, full_width, 0.0, &mut y);
            y += 4.0;
            if apply {
                row.button.setFrame(NSRect::new(
                    NSPoint::new(14.0, y),
                    NSSize::new(full_width, 28.0),
                ));
            }
            y += 34.0;
        }
        y
    }

    pub(crate) fn sync_extensions(&mut self, settings: &[(String, LifecycleSettingView)]) {
        if self.settings_snapshot == settings {
            return;
        }
        self.settings_snapshot = settings.to_vec();
        let mut old = std::mem::take(&mut self.extension_rows);
        for (provider, setting) in settings {
            let row = if let Some(index) = old
                .iter()
                .position(|row| row.provider == *provider && row.key == setting.key)
            {
                old.remove(index)
            } else {
                let toggle: Retained<NSSwitch> = unsafe {
                    msg_send![NSSwitch::alloc(self.mtm), initWithFrame: NSRect::default()]
                };
                toggle.setControlSize(NSControlSize::Small);
                toggle.setRefusesFirstResponder(false);
                unsafe {
                    toggle.setTarget(Some(&*self.target));
                    toggle.setAction(Some(sel!(toggleIntegrationSetting:)));
                }
                let setting_label = label("", 12.0, true, false, self.mtm);
                let help = label("", 10.5, false, true, self.mtm);
                for child in [&*toggle as &NSView, &*setting_label, &*help] {
                    self.extensions_view.addSubview(child);
                }
                SettingRow {
                    provider: provider.clone(),
                    key: setting.key.clone(),
                    label: setting_label,
                    help,
                    toggle,
                }
            };
            row.label
                .setStringValue(&NSString::from_str(&setting.label));
            row.help.setStringValue(&NSString::from_str(&setting.help));
            row.toggle.setIdentifier(Some(&NSString::from_str(
                &serde_json::to_string(&(provider, &setting.key)).expect("setting identifier"),
            )));
            row.toggle.setState(if setting.value {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            row.toggle
                .setEnabled(self.extensions_available && setting.enabled);
            accessibility(&row.toggle, &setting.label);
            accessibility(&row.help, &setting.help);
            self.extension_rows.push(row);
        }
        for row in old {
            row.label.removeFromSuperview();
            row.help.removeFromSuperview();
            row.toggle.removeFromSuperview();
        }
        self.extensions_empty
            .setHidden(!self.extension_rows.is_empty() || !self.action_rows.is_empty());
    }

    pub(crate) fn sync_actions(&mut self, actions: &[(String, LifecycleActionView)]) {
        if self.actions_snapshot == actions {
            return;
        }
        self.actions_snapshot = actions.to_vec();
        let mut old = std::mem::take(&mut self.action_rows);
        for (provider, action) in actions {
            let row = if let Some(index) = old
                .iter()
                .position(|row| row.provider == *provider && row.key == action.key)
            {
                old.remove(index)
            } else {
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str(""),
                        Some(&*self.target),
                        Some(sel!(activateIntegrationAction:)),
                        self.mtm,
                    )
                };
                let help = label("", 10.5, false, true, self.mtm);
                self.extensions_view.addSubview(&button);
                self.extensions_view.addSubview(&help);
                ActionRow {
                    provider: provider.clone(),
                    key: action.key.clone(),
                    help,
                    button,
                }
            };
            row.button.setTitle(&NSString::from_str(&action.label));
            row.button
                .setEnabled(self.extensions_available && action.enabled);
            row.button.setIdentifier(Some(&NSString::from_str(
                &serde_json::to_string(&(provider, &action.key)).expect("action identifier"),
            )));
            row.help.setStringValue(&NSString::from_str(&action.help));
            accessibility(&row.button, &action.label);
            accessibility(&row.help, &action.help);
            self.action_rows.push(row);
        }
        for row in old {
            row.button.removeFromSuperview();
            row.help.removeFromSuperview();
        }
        self.extensions_empty
            .setHidden(!self.extension_rows.is_empty() || !self.action_rows.is_empty());
    }

    pub(crate) fn set_locale(&mut self, locale: UiLocale) {
        self.locale = locale;
        for (field, message) in [
            (&self.heading, Message::LifecycleHeading),
            (&self.auto_label, Message::LifecycleAutoStart),
        ] {
            field.setStringValue(&NSString::from_str(text(locale, message)));
        }
        let empty = text(locale, Message::SettingsNoAdditionalOptions);
        self.extensions_empty
            .setStringValue(&NSString::from_str(empty));
        accessibility(&self.extensions_empty, empty);
        accessibility(&self.auto_switch, text(locale, Message::LifecycleAutoStart));
        let help = NSString::from_str(text(locale, Message::LifecycleAutoStartHelp));
        self.auto_switch.setToolTip(Some(&help));
        unsafe {
            let _: () = msg_send![&*self.auto_switch, setAccessibilityHelp: Some(&*help)];
        }
        self.error_details.setTitle(&NSString::from_str(text(
            locale,
            Message::SettingsDetailsLink,
        )));
        accessibility(
            &self.error_details,
            text(locale, Message::ConnectionDiagnostics),
        );
        self.display_error();
    }

    pub(crate) fn sync(&self, settings: LifecycleSettings) {
        self.auto_switch.setState(if settings.auto_start {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }

    pub(crate) fn value(&self, key: LifecycleSetting) -> bool {
        match key {
            LifecycleSetting::AutoStart => self.auto_switch.state() == NSControlStateValueOn,
        }
    }

    pub(crate) fn saved(&mut self, settings: LifecycleSettings) {
        self.error_detail = None;
        self.display_error();
        self.sync(settings);
    }

    pub(crate) fn failed(&mut self, settings: LifecycleSettings, detail: String) {
        self.sync(settings);
        self.error_detail = Some(detail);
        self.display_error();
    }

    pub(crate) fn error_detail(&self) -> Option<&str> {
        self.error_detail.as_deref()
    }

    pub(crate) fn extension_setting_enabled(&self, provider: &str, key: &str) -> bool {
        self.extensions_available
            && self
                .extension_rows
                .iter()
                .any(|row| row.provider == provider && row.key == key && row.toggle.isEnabled())
    }

    pub(crate) fn extension_action_enabled(&self, provider: &str, key: &str) -> bool {
        self.extensions_available
            && self
                .action_rows
                .iter()
                .any(|row| row.provider == provider && row.key == key && row.button.isEnabled())
    }

    pub(crate) fn set_extensions_available(&mut self, available: bool) {
        self.extensions_available = available;
        for row in &self.extension_rows {
            let enabled = self
                .settings_snapshot
                .iter()
                .find(|(provider, setting)| provider == &row.provider && setting.key == row.key)
                .is_some_and(|(_, setting)| setting.enabled);
            row.toggle.setEnabled(available && enabled);
        }
        for row in &self.action_rows {
            let enabled = self
                .actions_snapshot
                .iter()
                .find(|(provider, action)| provider == &row.provider && action.key == row.key)
                .is_some_and(|(_, action)| action.enabled);
            row.button.setEnabled(available && enabled);
        }
    }

    fn display_error(&self) {
        let message = if self.error_detail.is_some() {
            text(self.locale, Message::LifecycleSaveFailure)
        } else {
            ""
        };
        self.error.setStringValue(&NSString::from_str(&message));
        self.error.setHidden(message.is_empty());
        self.error_details.setHidden(message.is_empty());
        accessibility(&self.error, &message);
    }
}
