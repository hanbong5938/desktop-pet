use crate::i18n::{text, Message, UiLocale};
use crate::lifecycle::{LifecycleSetting, LifecycleSettings};
use herdr_update_coordinator::ObservationMode;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSControlSize, NSControlStateValueOff, NSControlStateValueOn, NSFont,
    NSSwitch, NSTextField, NSView,
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
    heading: Retained<NSTextField>,
    auto_label: Retained<NSTextField>,
    auto_help: Retained<NSTextField>,
    auto_switch: Retained<NSSwitch>,
    exit_label: Retained<NSTextField>,
    exit_help: Retained<NSTextField>,
    exit_switch: Retained<NSSwitch>,
    quit_help: Retained<NSTextField>,
    persistent_help: Retained<NSTextField>,
    observation_host: ObservationMode,
    error: Retained<NSTextField>,
    locale: UiLocale,
    error_detail: Option<String>,
}

impl LifecycleSettingsCard {
    pub(crate) fn new(locale: UiLocale, target: &AnyObject, mtm: MainThreadMarker) -> Self {
        let view = CardView::new(mtm);
        unsafe {
            let _: () = msg_send![&*view, setAccessibilityElement: false];
        }
        let heading = label("", 12.5, true, false, mtm);
        let auto_label = label("", 12.0, true, false, mtm);
        let auto_help = label("", 10.5, false, true, mtm);
        let exit_label = label("", 12.0, true, false, mtm);
        let exit_help = label("", 10.5, false, true, mtm);
        let quit_help = label("", 10.5, false, true, mtm);
        let persistent_help = label("", 10.5, false, true, mtm);
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
        let exit_switch = make_switch(sel!(toggleLifecycleExit:));
        for child in [
            &*heading as &NSView,
            &*auto_label,
            &*auto_help,
            &*auto_switch,
            &*exit_label,
            &*exit_help,
            &*exit_switch,
            &*quit_help,
            &*persistent_help,
            &*error,
        ] {
            view.addSubview(child);
        }
        let mut card = Self {
            view,
            heading,
            auto_label,
            auto_help,
            auto_switch,
            exit_label,
            exit_help,
            exit_switch,
            quit_help,
            persistent_help,
            observation_host: ObservationMode::Both,
            error,
            locale,
            error_detail: None,
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
        y += 2.0;
        place(&self.auto_help, full_width, 0.0, &mut y);
        y += 6.0;
        if self.observation_host == ObservationMode::Herdr {
            let exit_y = y;
            place(&self.exit_label, text_width, 32.0, &mut y);
            if apply {
                self.exit_switch.setFrame(NSRect::new(
                    NSPoint::new(width - 58.0, exit_y),
                    NSSize::new(46.0, 30.0),
                ));
            }
            y += 2.0;
            place(&self.exit_help, full_width, 0.0, &mut y);
        } else {
            place(&self.persistent_help, full_width, 0.0, &mut y);
        }
        y += 6.0;
        place(&self.quit_help, full_width, 0.0, &mut y);
        if !self.error.isHidden() {
            y += 6.0;
            place(&self.error, full_width, 0.0, &mut y);
        }
        y + 10.0
    }

    pub(crate) fn set_observation_host(&mut self, host: ObservationMode) {
        self.observation_host = host;
        let herdr_only = host == ObservationMode::Herdr;
        self.exit_label.setHidden(!herdr_only);
        self.exit_help.setHidden(!herdr_only);
        self.exit_switch.setHidden(!herdr_only);
        self.exit_switch.setEnabled(herdr_only);
        self.persistent_help.setHidden(herdr_only);
    }

    pub(crate) fn set_locale(&mut self, locale: UiLocale) {
        self.locale = locale;
        for (field, message) in [
            (&self.heading, Message::LifecycleHeading),
            (&self.auto_label, Message::LifecycleAutoStart),
            (&self.auto_help, Message::LifecycleAutoStartHelp),
            (&self.exit_label, Message::LifecycleExit),
            (&self.exit_help, Message::LifecycleExitHelp),
            (&self.quit_help, Message::LifecycleQuitHelp),
            (&self.persistent_help, Message::LifecyclePersistentHelp),
        ] {
            field.setStringValue(&NSString::from_str(text(locale, message)));
        }
        accessibility(&self.auto_switch, text(locale, Message::LifecycleAutoStart));
        accessibility(&self.exit_switch, text(locale, Message::LifecycleExit));
        accessibility(
            &self.auto_help,
            text(locale, Message::LifecycleAutoStartHelp),
        );
        accessibility(&self.exit_help, text(locale, Message::LifecycleExitHelp));
        accessibility(
            &self.persistent_help,
            text(locale, Message::LifecyclePersistentHelp),
        );
        self.set_observation_host(self.observation_host);
        self.display_error();
    }

    pub(crate) fn sync(&self, settings: LifecycleSettings) {
        self.auto_switch.setState(if settings.auto_start {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.exit_switch.setState(if settings.exit_with_herdr {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }

    pub(crate) fn value(&self, key: LifecycleSetting) -> bool {
        match key {
            LifecycleSetting::AutoStart => self.auto_switch.state() == NSControlStateValueOn,
            LifecycleSetting::ExitWithHerdr => self.exit_switch.state() == NSControlStateValueOn,
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

    fn display_error(&self) {
        let message = self
            .error_detail
            .as_ref()
            .map(|detail| {
                format!(
                    "{}: {detail}",
                    text(self.locale, Message::LifecycleSaveFailure)
                )
            })
            .unwrap_or_default();
        self.error.setStringValue(&NSString::from_str(&message));
        self.error.setHidden(message.is_empty());
        accessibility(&self.error, &message);
    }
}
