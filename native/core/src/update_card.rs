use crate::i18n::{text, Message, UiLocale};
use crate::ui::MenuTarget;
use desktop_pet_update_coordinator::{CheckStatus, ReleaseCandidate, UpdateAction};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBezelStyle, NSBezierPath, NSButton, NSButtonType, NSColor, NSControlSize,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSLineBreakMode, NSSwitch, NSTextField,
    NSView,
};
use objc2_foundation::{
    NSDate, NSDateFormatter, NSDateFormatterStyle, NSLocale, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, NSTimeZone,
};

const UPDATE_HEADER_HEIGHT: f64 = 24.0;

// SAFETY: AppKit owns this decorative view and all calls are on the main thread.
define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "OMPetAppUpdateCardView"]
    #[ivars = f64]
    struct CardView;
    unsafe impl NSObjectProtocol for CardView {}
    impl CardView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let frame = self.bounds();
            let top = *self.ivars();
            let bounds = NSRect::new(
                NSPoint::new(0.0, top),
                NSSize::new(frame.size.width, (frame.size.height - top).max(0.0)),
            );
            NSColor::colorWithSRGBRed_green_blue_alpha(0.161, 0.161, 0.176, 1.0).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, 10.0, 10.0).fill();
            NSColor::colorWithSRGBRed_green_blue_alpha(0.28, 0.28, 0.32, 0.5).setStroke();
            let border = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                NSRect::new(
                    NSPoint::new(0.5, top + 0.5),
                    NSSize::new((bounds.size.width - 1.0).max(1.0), (bounds.size.height - 1.0).max(1.0)),
                ),
                9.5,
                9.5,
            );
            border.setLineWidth(1.0);
            border.stroke();
        }
    }
);

impl CardView {
    fn new(top: f64, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(top);
        unsafe { msg_send![super(this), initWithFrame: NSRect::default()] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateCardPrimaryAction {
    Update,
    Migrate,
    Relocate,
    Recover,
    RecoverStart,
    RecoverStoppedOriginal,
    ShowProgress,
}

#[derive(Clone)]
pub(crate) struct UpdateCardModel {
    pub(crate) auto_check: bool,
    pub(crate) receive_beta_updates: bool,
    pub(crate) busy: bool,
    pub(crate) settings_locked: bool,
    // Tint only; status_text carries the precise localized operation/check outcome.
    pub(crate) status: CheckStatus,
    pub(crate) status_text: String,
    pub(crate) running_version: String,
    pub(crate) source: String,
    pub(crate) detail: String,
    pub(crate) history: String,
    pub(crate) diagnostics: String,
    pub(crate) last_checked: Option<u64>,
    pub(crate) action: Option<UpdateAction>,
    pub(crate) primary_action: Option<UpdateCardPrimaryAction>,
    pub(crate) release: Option<ReleaseCandidate>,
    pub(crate) download_url: Option<String>,
    pub(crate) migration_available: bool,
    pub(crate) migration_visible: bool,
    pub(crate) migration_title: String,
    pub(crate) relocation_available: bool,
    pub(crate) relocation_title: String,
    pub(crate) relocation_detail: String,
    pub(crate) relocation_summary: String,
}

impl UpdateCardModel {
    pub(crate) fn retain_completed_migration(&mut self, operation_id: &str, locale: UiLocale) {
        self.migration_visible = false;
        self.migration_available = false;
        self.history.push_str(&format!(
            "\n{} · {}",
            operation_id,
            text(locale, Message::MigrationRetired)
        ));
    }
}

pub(crate) struct AppUpdateCard {
    view: Retained<CardView>,
    settings_view: Retained<CardView>,
    details_view: Retained<CardView>,
    heading: Retained<NSTextField>,
    auto_label: Retained<NSTextField>,
    auto_help: Retained<NSTextField>,
    auto_switch: Retained<NSSwitch>,
    beta_summary: Retained<NSTextField>,
    beta_label: Retained<NSTextField>,
    beta_help: Retained<NSTextField>,
    beta_switch: Retained<NSSwitch>,
    status_icon: Retained<NSTextField>,
    status: Retained<NSTextField>,
    running: Retained<NSTextField>,
    source: Retained<NSTextField>,
    release: Retained<NSTextField>,
    detail: Retained<NSTextField>,
    compact_detail: Retained<NSTextField>,
    relocation_detail: Retained<NSTextField>,
    history: Retained<NSTextField>,
    diagnostics: Retained<NSTextField>,
    check: Retained<NSButton>,
    apply: Retained<NSButton>,
    download: Retained<NSButton>,
    migration: Retained<NSButton>,
    last_checked: Retained<NSTextField>,
    locale: UiLocale,
    // All three surfaces are projected from the final (including migration overlay) model.
    model: Option<UpdateCardModel>,
}

fn basic_status(model: &UpdateCardModel) -> &str {
    if model.relocation_summary.is_empty() {
        &model.status_text
    } else {
        &model.relocation_summary
    }
}

impl AppUpdateCard {
    pub(crate) fn new(locale: UiLocale, target: &MenuTarget, mtm: MainThreadMarker) -> Self {
        let view = CardView::new(UPDATE_HEADER_HEIGHT, mtm);
        let settings_view = CardView::new(0.0, mtm);
        let details_view = CardView::new(0.0, mtm);
        for root in [&view, &settings_view, &details_view] {
            unsafe {
                let _: () = msg_send![&**root, setAccessibilityElement: false];
            }
        }
        let heading = label(12.5, true, false, mtm);
        let auto_label = label(12.0, true, false, mtm);
        let auto_help = label(10.5, false, true, mtm);
        let make_switch = |action| {
            let switch: Retained<NSSwitch> = unsafe {
                msg_send![NSSwitch::alloc(mtm), initWithFrame: rect(0.0, 0.0, 46.0, 30.0)]
            };
            switch.setControlSize(NSControlSize::Small);
            switch.setRefusesFirstResponder(false);
            unsafe {
                switch.setTarget(Some(target));
                switch.setAction(Some(action));
            }
            switch
        };
        let auto_switch = make_switch(sel!(toggleUpdateAutoCheck:));
        let beta_summary = label(10.5, false, true, mtm);
        let beta_label = label(12.0, true, false, mtm);
        let beta_help = label(10.5, false, true, mtm);
        let beta_switch = make_switch(sel!(toggleUpdateBeta:));
        let status_icon = label(13.0, true, false, mtm);
        unsafe {
            let _: () = msg_send![&*status_icon, setAccessibilityElement: false];
        }
        let status = label(11.5, true, false, mtm);
        let running = label(10.5, false, true, mtm);
        let source = diagnostic_label(10.5, mtm);
        let release = diagnostic_label(10.5, mtm);
        let detail = diagnostic_label(10.5, mtm);
        let compact_detail = diagnostic_label(10.5, mtm);
        compact_detail.setMaximumNumberOfLines(5);
        compact_detail.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        compact_detail.setHidden(true);
        let relocation_detail = diagnostic_label(10.5, mtm);
        let history = diagnostic_label(10.0, mtm);
        let diagnostics = diagnostic_label(10.0, mtm);
        let last_checked = diagnostic_label(10.0, mtm);
        for field in [
            &source,
            &release,
            &detail,
            &relocation_detail,
            &history,
            &diagnostics,
            &last_checked,
        ] {
            field.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(10.5, 0.0)));
        }
        let check = button(target, sel!(checkAppUpdate:), mtm);
        let apply = button(target, sel!(applyAppUpdate:), mtm);
        let download = button(target, sel!(downloadAppUpdate:), mtm);
        let migration = button(target, sel!(migrateLegacyInstallation:), mtm);
        for child in [
            &*heading as &NSView,
            &*running,
            &*status_icon,
            &*status,
            &*compact_detail,
            &*beta_summary,
            &*check,
            &*apply,
            &*download,
            &*migration,
        ] {
            view.addSubview(child);
        }
        for child in [
            &*auto_label as &NSView,
            &*auto_help,
            &*auto_switch,
            &*beta_label,
            &*beta_help,
            &*beta_switch,
        ] {
            settings_view.addSubview(child);
        }
        for child in [
            &*source as &NSView,
            &*release,
            &*last_checked,
            &*detail,
            &*relocation_detail,
            &*history,
            &*diagnostics,
        ] {
            details_view.addSubview(child);
        }
        let mut card = Self {
            view,
            settings_view,
            details_view,
            heading,
            auto_label,
            auto_help,
            auto_switch,
            beta_summary,
            beta_label,
            beta_help,
            beta_switch,
            status_icon,
            status,
            running,
            source,
            release,
            detail,
            compact_detail,
            relocation_detail,
            history,
            diagnostics,
            check,
            apply,
            download,
            migration,
            last_checked,
            locale,
            model: None,
        };
        card.set_locale(locale);
        card
    }

    pub(crate) fn view(&self) -> &NSView {
        &self.view
    }
    pub(crate) fn settings_view(&self) -> &NSView {
        &self.settings_view
    }
    pub(crate) fn details_view(&self) -> &NSView {
        &self.details_view
    }

    pub(crate) fn set_locale(&mut self, locale: UiLocale) {
        self.locale = locale;
        for (field, message) in [
            (&self.heading, Message::AppUpdateTitle),
            (&self.auto_label, Message::AppUpdateAutoCheck),
            (&self.auto_help, Message::AppUpdateAutomaticHelp),
            (&self.beta_label, Message::AppUpdateReceiveBeta),
            (&self.beta_help, Message::AppUpdateBetaHelp),
            (&self.beta_summary, Message::AppUpdateBetaEnabledSummary),
        ] {
            set_text(field, text(locale, message));
            accessibility(field, text(locale, message));
        }
        accessibility(&self.auto_switch, text(locale, Message::AppUpdateAutoCheck));
        accessibility_help(
            &self.auto_switch,
            text(locale, Message::AppUpdateAutomaticHelp),
        );
        accessibility(
            &self.beta_switch,
            text(locale, Message::AppUpdateReceiveBeta),
        );
        accessibility_help(&self.beta_switch, text(locale, Message::AppUpdateBetaHelp));
        self.download.setTitle(&NSString::from_str(text(
            locale,
            Message::AppUpdateDownloadRelease,
        )));
        accessibility(
            &self.download,
            text(locale, Message::AppUpdateDownloadRelease),
        );
        if let Some(model) = self.model.take() {
            self.refresh(&model);
        } else {
            set_text(&self.status, text(locale, Message::AppUpdateNotChecked));
            accessibility(&self.status, text(locale, Message::AppUpdateNotChecked));
            self.check.setTitle(&NSString::from_str(text(
                locale,
                Message::AppUpdateCheckNow,
            )));
            accessibility(&self.check, text(locale, Message::AppUpdateCheckNow));
            for field in [&self.running, &self.beta_summary] {
                field.setHidden(true);
            }
            for action in [&self.apply, &self.download, &self.migration] {
                action.setHidden(true);
            }
            set_text(&self.history, text(locale, Message::SettingsHistoryEmpty));
            set_text(
                &self.diagnostics,
                text(locale, Message::SettingsDetailsEmpty),
            );
        }
    }

    pub(crate) fn refresh(&mut self, model: &UpdateCardModel) {
        self.auto_switch.setState(if model.auto_check {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.beta_switch.setState(if model.receive_beta_updates {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.auto_switch
            .setEnabled(!model.busy && !model.settings_locked);
        self.beta_switch
            .setEnabled(!model.busy && !model.settings_locked);
        let check_title = text(
            self.locale,
            if model.last_checked.is_some() || model.status != CheckStatus::NotChecked {
                Message::AppUpdateCheckAgain
            } else {
                Message::AppUpdateCheckNow
            },
        );
        self.check.setTitle(&NSString::from_str(check_title));
        accessibility(&self.check, check_title);
        self.check.setEnabled(!model.busy);
        let (symbol, tint) = match model.status {
            CheckStatus::Current | CheckStatus::Installed | CheckStatus::Applied => {
                ("✓", NSColor::systemGreenColor())
            }
            CheckStatus::Available | CheckStatus::Local => ("●", NSColor::systemBlueColor()),
            CheckStatus::Failed | CheckStatus::Unsupported => ("!", NSColor::systemOrangeColor()),
            CheckStatus::NotChecked | CheckStatus::Checking => ("○", muted()),
        };
        set_text(&self.status_icon, symbol);
        self.status_icon.setTextColor(Some(&tint));
        set_text(&self.status, basic_status(model));
        accessibility(&self.status, basic_status(model));
        let running = if model.running_version.is_empty() {
            String::new()
        } else {
            format!(
                "{}: {}",
                text(self.locale, Message::AppUpdateRunning),
                model.running_version
            )
        };
        set_text(&self.running, &running);
        accessibility(&self.running, &running);
        self.running.setHidden(running.is_empty());
        self.beta_summary.setHidden(!model.receive_beta_updates);
        set_text(&self.compact_detail, &model.detail);
        accessibility(&self.compact_detail, &model.detail);
        accessibility_help(
            &self.compact_detail,
            &format!(
                "{}: {}",
                text(self.locale, Message::UpdateDetails),
                model.detail
            ),
        );
        self.compact_detail
            .setToolTip(Some(&NSString::from_str(&model.detail)));
        self.compact_detail.setHidden(model.detail.is_empty());

        let source = format!(
            "{}: {}",
            text(self.locale, Message::AppUpdateSource),
            model.source
        );
        let release = model.release.as_ref().map_or_else(String::new, |release| {
            format!("{} ({})", release.version, release.tag)
        });
        let checked = match model
            .last_checked
            .and_then(|seconds| format_checked_at(seconds, self.locale))
        {
            Some(date) => format!(
                "{}: {}",
                text(self.locale, Message::AppUpdateLastChecked),
                date
            ),
            None => text(self.locale, Message::AppUpdateNeverChecked).to_owned(),
        };
        for (field, value, hide_empty) in [
            (&self.source, source.as_str(), false),
            (&self.release, release.as_str(), true),
            (&self.last_checked, checked.as_str(), false),
            (&self.detail, model.detail.as_str(), true),
            (
                &self.relocation_detail,
                model.relocation_detail.as_str(),
                true,
            ),
            (
                &self.history,
                if model.history.is_empty() {
                    text(self.locale, Message::SettingsHistoryEmpty)
                } else {
                    &model.history
                },
                false,
            ),
            (
                &self.diagnostics,
                if model.diagnostics.is_empty() {
                    text(self.locale, Message::SettingsDetailsEmpty)
                } else {
                    &model.diagnostics
                },
                false,
            ),
        ] {
            set_text(field, value);
            accessibility(field, value);
            field.setHidden(hide_empty && value.is_empty());
        }
        let detail_color = if model.status == CheckStatus::Failed {
            NSColor::systemRedColor()
        } else {
            muted()
        };
        self.detail.setTextColor(Some(&detail_color));
        self.compact_detail.setTextColor(Some(&detail_color));

        unsafe {
            self.apply.setAction(Some(match model.primary_action {
                Some(UpdateCardPrimaryAction::Migrate) => sel!(migrateLegacyInstallation:),
                Some(UpdateCardPrimaryAction::Relocate) => sel!(relocateStandaloneInstallation:),
                Some(UpdateCardPrimaryAction::Recover) => sel!(recoverAppUpdate:),
                Some(UpdateCardPrimaryAction::RecoverStart) => sel!(startAppUpdateRecovery:),
                Some(UpdateCardPrimaryAction::RecoverStoppedOriginal) => {
                    sel!(recoverStoppedOriginalAppUpdate:)
                }
                Some(UpdateCardPrimaryAction::ShowProgress) => sel!(showAppUpdateRecoveryProgress:),
                Some(UpdateCardPrimaryAction::Update) | None => sel!(applyAppUpdate:),
            }));
        }
        let action_title = match model.primary_action {
            Some(UpdateCardPrimaryAction::Migrate) => Some(model.migration_title.as_str()),
            Some(UpdateCardPrimaryAction::Relocate) => Some(model.relocation_title.as_str()),
            Some(UpdateCardPrimaryAction::Recover) => {
                Some(text(self.locale, Message::AppUpdateRecover))
            }
            Some(UpdateCardPrimaryAction::RecoverStart) => {
                Some(text(self.locale, Message::AppUpdateRecoveryStart))
            }
            Some(UpdateCardPrimaryAction::RecoverStoppedOriginal) => {
                Some(text(self.locale, Message::AppUpdateRecoveryStop))
            }
            Some(UpdateCardPrimaryAction::ShowProgress) => {
                Some(text(self.locale, Message::AppUpdateRecoveryProgress))
            }
            Some(UpdateCardPrimaryAction::Update) => model.action.as_ref().map(|action| {
                text(
                    self.locale,
                    match action {
                        UpdateAction::StandaloneReplace | UpdateAction::HomebrewUpgrade => {
                            Message::AppUpdateInstall
                        }
                        UpdateAction::SourceRebuild => Message::AppUpdateRebuild,
                        UpdateAction::ApplyInstalled => Message::AppUpdateApply,
                        #[cfg(desktop_pet_legacy)]
                        UpdateAction::HerdrReinstall => Message::AppUpdateInstall,
                    },
                )
            }),
            None => None,
        };
        self.apply.setHidden(model.primary_action.is_none());
        if let Some(title) = action_title {
            self.apply.setTitle(&NSString::from_str(title));
            accessibility(&self.apply, title);
        }
        let action_help =
            if model.primary_action == Some(UpdateCardPrimaryAction::RecoverStoppedOriginal) {
                text(self.locale, Message::AppUpdateRecoveryStopHelp)
            } else {
                ""
            };
        let action_help = (!action_help.is_empty()).then(|| NSString::from_str(action_help));
        self.apply.setToolTip(action_help.as_deref());
        unsafe {
            let _: () = msg_send![&*self.apply, setAccessibilityHelp: action_help.as_deref()];
        }
        let recovery = matches!(
            model.primary_action,
            Some(
                UpdateCardPrimaryAction::Recover
                    | UpdateCardPrimaryAction::RecoverStart
                    | UpdateCardPrimaryAction::RecoverStoppedOriginal
                    | UpdateCardPrimaryAction::ShowProgress
            )
        );
        self.apply.setEnabled(match model.primary_action {
            Some(UpdateCardPrimaryAction::ShowProgress) => true,
            Some(
                UpdateCardPrimaryAction::Recover
                | UpdateCardPrimaryAction::RecoverStart
                | UpdateCardPrimaryAction::RecoverStoppedOriginal,
            ) => !model.busy,
            Some(UpdateCardPrimaryAction::Update) => {
                !model.busy && !model.settings_locked && model.action.is_some()
            }
            Some(UpdateCardPrimaryAction::Migrate) => {
                !model.busy && !model.settings_locked && model.migration_available
            }
            Some(UpdateCardPrimaryAction::Relocate) => {
                !model.busy && !model.settings_locked && model.relocation_available
            }
            None => false,
        });
        self.check
            .setEnabled(!model.busy && !model.settings_locked && !recovery);
        self.download
            .setHidden(model.download_url.is_none() || recovery);
        self.download
            .setEnabled(!model.busy && !model.settings_locked && model.download_url.is_some());
        self.migration
            .setTitle(&NSString::from_str(&model.migration_title));
        accessibility(&self.migration, &model.migration_title);
        self.migration.setHidden(
            !model.migration_visible
                || recovery
                || model.settings_locked
                || model.primary_action == Some(UpdateCardPrimaryAction::Migrate),
        );
        self.migration
            .setEnabled(!model.busy && model.migration_available);
        self.model = Some(model.clone());
    }

    pub(crate) fn height(&self, width: f64) -> f64 {
        self.layout_contents(width, false)
    }
    pub(crate) fn layout(&self, frame: NSRect) {
        self.view.setFrame(frame);
        self.layout_contents(frame.size.width, true);
    }
    pub(crate) fn settings_height(&self, width: f64) -> f64 {
        self.layout_settings_contents(width, false)
    }
    pub(crate) fn layout_settings(&self, frame: NSRect) {
        self.settings_view.setFrame(frame);
        self.layout_settings_contents(frame.size.width, true);
    }
    pub(crate) fn details_height(&self, width: f64) -> f64 {
        self.layout_details_contents(width, false)
    }
    pub(crate) fn layout_details(&self, frame: NSRect) {
        self.details_view.setFrame(frame);
        self.layout_details_contents(frame.size.width, true);
    }

    fn layout_contents(&self, width: f64, apply: bool) -> f64 {
        let full = (width - 28.0).max(1.0);
        if apply {
            self.status_icon.setFrame(rect(2.0, 0.0, 18.0, 18.0));
            self.heading
                .setFrame(rect(24.0, 0.0, (width - 24.0).max(1.0), 19.0));
        }
        let mut y = UPDATE_HEADER_HEIGHT + 8.0;
        if !self.running.isHidden() {
            place(&self.running, 14.0, full, 14.0, &mut y, apply);
            y += 2.0;
        }
        let status_height = measured(&self.status, full).clamp(18.0, 54.0);
        if apply {
            self.status.setFrame(rect(14.0, y, full, status_height));
        }
        y += status_height;
        if !self.compact_detail.isHidden() {
            y += 3.0;
            let detail_height = measured(&self.compact_detail, full).clamp(14.0, 76.0);
            if apply {
                self.compact_detail
                    .setFrame(rect(14.0, y, full, detail_height));
            }
            y += detail_height;
        }
        if !self.beta_summary.isHidden() {
            y += 2.0;
            place(&self.beta_summary, 14.0, full, 14.0, &mut y, apply);
        }
        y += 5.0;
        let mut x = 14.0;
        let mut row_y = y;
        for button in [&self.check, &self.apply, &self.download, &self.migration] {
            if button.isHidden() {
                continue;
            }
            let button_width = button_width(button, full);
            if x > 14.0 && x + button_width > 14.0 + full {
                x = 14.0;
                row_y += 31.0;
            }
            if apply {
                button.setFrame(rect(x, row_y, button_width, 25.0));
            }
            x += button_width + 8.0;
        }
        row_y + 33.0
    }

    fn layout_settings_contents(&self, width: f64, apply: bool) -> f64 {
        let full = (width - 28.0).max(1.0);
        let beside_switch = (width - 82.0).max(1.0);
        let mut y = 12.0;
        for (label, help, switch) in [
            (&self.auto_label, &self.auto_help, &self.auto_switch),
            (&self.beta_label, &self.beta_help, &self.beta_switch),
        ] {
            let label_height = measured(label, beside_switch);
            let height = label_height.max(22.0);
            if apply {
                label.setFrame(rect(
                    14.0,
                    y + (height - label_height) * 0.5,
                    beside_switch,
                    label_height,
                ));
                switch.setFrame(rect((width - 58.0).max(0.0), y - 4.0, 46.0, 30.0));
            }
            y += height + 2.0;
            place(help, 14.0, full, 14.0, &mut y, apply);
            y += 6.0;
        }
        y + 4.0
    }

    fn layout_details_contents(&self, width: f64, apply: bool) -> f64 {
        let full = (width - 28.0).max(1.0);
        let mut y = 8.0;
        for field in [
            &self.source,
            &self.release,
            &self.last_checked,
            &self.detail,
            &self.relocation_detail,
            &self.history,
            &self.diagnostics,
        ] {
            if field.isHidden() {
                continue;
            }
            place(field, 14.0, full, 14.0, &mut y, apply);
            y += 2.0;
        }
        y + 6.0
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn place(field: &NSTextField, x: f64, width: f64, minimum: f64, y: &mut f64, apply: bool) {
    let height = measured(field, width).max(minimum);
    if apply {
        field.setFrame(rect(x, *y, width, height));
    }
    *y += height;
}

fn measured(field: &NSTextField, width: f64) -> f64 {
    field
        .cell()
        .expect("wrapping label cell")
        .cellSizeForBounds(rect(0.0, 0.0, width.max(1.0), 100_000.0))
        .height
        .ceil()
}

fn button_width(button: &NSButton, available: f64) -> f64 {
    (button.cell().expect("button cell").cellSize().width + 18.0)
        .max(82.0)
        .min(available)
}

fn label(size: f64, bold: bool, muted_color: bool, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
    let font = if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    let color = if muted_color {
        muted()
    } else {
        NSColor::colorWithSRGBRed_green_blue_alpha(0.95, 0.95, 0.97, 1.0)
    };
    field.setTextColor(Some(&color));
    field.setMaximumNumberOfLines(0);
    field.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    field
}

fn diagnostic_label(size: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = label(size, false, true, mtm);
    field.setEditable(false);
    field.setSelectable(true);
    field.setRefusesFirstResponder(false);
    field.setLineBreakMode(NSLineBreakMode::ByCharWrapping);
    field
}

fn muted() -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(0.60, 0.61, 0.66, 1.0)
}

fn button(
    target: &MenuTarget,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(""), None, None, mtm)
    };
    button.setButtonType(NSButtonType::MomentaryPushIn);
    button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    button.setBordered(true);
    button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    button.setContentTintColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
        0.95, 0.95, 0.97, 1.0,
    )));
    button.setRefusesFirstResponder(false);
    unsafe {
        button.setTarget(Some(target));
        button.setAction(Some(action));
    }
    button
}

fn set_text(field: &NSTextField, value: &str) {
    if field.stringValue().to_string() != value {
        field.setStringValue(&NSString::from_str(value));
    }
}

fn accessibility(view: &NSView, value: &str) {
    let label = NSString::from_str(value);
    unsafe {
        let _: () = msg_send![view, setAccessibilityLabel: Some(&*label)];
    }
}
fn accessibility_help(view: &NSView, value: &str) {
    let help = NSString::from_str(value);
    unsafe {
        let _: () = msg_send![view, setAccessibilityHelp: Some(&*help)];
    }
}

fn format_checked_at(seconds: u64, locale: UiLocale) -> Option<String> {
    // A persisted UNIX timestamp is historical evidence, never synthesized from now.
    // NSDate uses local time zone, and the formatter's language follows the card.
    let seconds = i64::try_from(seconds).ok()?;
    let date = NSDate::dateWithTimeIntervalSince1970(seconds as f64);
    let formatter = NSDateFormatter::new();
    formatter.setLocale(Some(&NSLocale::localeWithLocaleIdentifier(
        &NSString::from_str(locale.tag()),
    )));
    formatter.setTimeZone(Some(&NSTimeZone::localTimeZone()));
    formatter.setDateStyle(NSDateFormatterStyle::MediumStyle);
    formatter.setTimeStyle(NSDateFormatterStyle::ShortStyle);
    Some(formatter.stringFromDate(&date).to_string())
}
