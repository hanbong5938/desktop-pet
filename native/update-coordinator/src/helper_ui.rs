//! The coordinator is outside the replaceable app and owns its own native window.
use desktop_pet_update_coordinator::migration::{
    MigrationPhase, MigrationPlan, MigrationPresentation,
};
use desktop_pet_update_coordinator::standalone_install::StandaloneInstallPlan;
use desktop_pet_update_coordinator::{identity, InstallOrigin, UpdatePlan};
use sha2::{Digest, Sha256};

fn migration_digest(plan: &MigrationPlan) -> Result<[u8; 32], String> {
    Ok(Sha256::digest(serde_json::to_vec(plan).map_err(|e| e.to_string())?).into())
}

fn migration_confirmation_response(response: isize) -> Result<bool, String> {
    match response {
        1000 => Ok(true),
        1001 => Ok(false),
        _ => Err(format!(
            "native migration consent ended without an explicit decision: {response}"
        )),
    }
}

fn install_digest(plan: &StandaloneInstallPlan) -> Result<[u8; 32], String> {
    Ok(Sha256::digest(serde_json::to_vec(plan).map_err(|e| e.to_string())?).into())
}

fn require_install_authorization(
    expected: Option<[u8; 32]>,
    actual: [u8; 32],
    visible: bool,
) -> Result<(), String> {
    if expected != Some(actual) {
        return Err("install window is not bound to this source/target/profile approval".into());
    }
    if !visible {
        return Err("independent native install window is not visible".into());
    }
    Ok(())
}

fn install_confirmation_response(
    expected: Option<[u8; 32]>,
    actual: [u8; 32],
    visible: bool,
    response: isize,
) -> Result<bool, String> {
    require_install_authorization(expected, actual, visible)?;
    match response {
        1000 => Ok(true),
        1001 => Ok(false),
        _ => Err(format!(
            "native install consent ended without an explicit decision: {response}"
        )),
    }
}

fn install_summary(
    plan: &StandaloneInstallPlan,
    locale: Locale,
    recovery: bool,
) -> Result<String, String> {
    let source = desktop_pet_update_coordinator::physical_install_root(plan.origin())?;
    let target = desktop_pet_update_coordinator::physical_install_root(plan.installed_origin())?;
    let relocation = !plan.purpose.is_upgrade();
    let (version_label, config_label, state_label, action) = match locale {
        Locale::Ko => (
            "버전",
            "설정",
            "상태",
            if recovery {
                "복구는 원래 승인만 조정합니다. 새 위치 이관 동의가 아니며 종료를 재전송하거나 이후의 중지를 무시하거나 새 재시작 권한을 부여하지 않습니다."
            } else if plan.running().is_some() {
                "Desktop Pet만 종료하고 이 정확한 프로필로 다시 시작합니다. 계속하기 전에 저장하지 않은 Pet 편집 내용을 저장하세요. Tern과 창, 터미널, 작업은 계속 유지됩니다."
            } else {
                "Desktop Pet은 중지된 상태를 유지합니다. Tern과 창, 터미널, 작업은 계속 유지됩니다."
            },
        ),
        Locale::En => (
            "Version",
            "Config",
            "State",
            if recovery {
                "Recovery only reconciles the original approval; it is not new relocation consent. It cannot replay Quit, override a later Stop, or grant new restart permission."
            } else if plan.running().is_some() {
                "Only Desktop Pet quits and restarts with this exact profile. Save any unsaved Pet edits before continuing. Tern, its windows, terminals and work stay running."
            } else {
                "Desktop Pet is stopped and will remain stopped. Tern, its windows, terminals and work stay running."
            },
        ),
        Locale::Ja => (
            "バージョン",
            "設定",
            "状態",
            if recovery {
                "復旧は元の承認内容のみを照合します。新たな移動の同意ではありません。終了の再送、後の停止の無視、新たな再起動権限の付与はできません。"
            } else if plan.running().is_some() {
                "Desktop Pet だけを終了し、この同じプロファイルで再起動します。続行する前に未保存の Pet の編集を保存してください。Tern、ウィンドウ、ターミナル、作業はそのまま維持されます。"
            } else {
                "Desktop Pet は停止したままです。Tern、ウィンドウ、ターミナル、作業はそのまま維持されます。"
            },
        ),
        Locale::ZhHans => (
            "版本",
            "设置",
            "状态",
            if recovery {
                "恢复仅核对原批准内容，不是新的迁移同意。不能再次发送退出、忽略之后的停止或授予新的重启权限。"
            } else if plan.running().is_some() {
                "仅退出 Desktop Pet 并使用完全相同的配置重启。继续前请保存未保存的 Pet 编辑。Tern、窗口、终端和工作保持运行。"
            } else {
                "Desktop Pet 将保持停止状态。Tern、窗口、终端和工作保持运行。"
            },
        ),
    };
    let version = if relocation {
        match locale {
            Locale::Ko => format!("{} (같은 버전 · 서명된 동일한 앱)", plan.candidate_version),
            Locale::En => format!(
                "{} (same version · identical signed app)",
                plan.candidate_version
            ),
            Locale::Ja => format!(
                "{}（同じバージョン・同一の署名済みアプリ）",
                plan.candidate_version
            ),
            Locale::ZhHans => format!(
                "{}（相同版本 · 完全相同的已签名应用）",
                plan.candidate_version
            ),
        }
    } else {
        format!("{} → {}", plan.original.app_version, plan.candidate_version)
    };
    Ok(format!(
        "{}\n→ {}\n{version_label}: {version}\n{config_label}: {}\n{state_label}: {}\n\n{action}",
        source.display(),
        target.display(),
        plan.profile.config_dir.display(),
        plan.profile.state_dir.display()
    ))
}

fn install_buttons(
    locale: Locale,
    relocation: bool,
    recovery: bool,
) -> (&'static str, &'static str, &'static str) {
    match (locale, relocation, recovery) {
        (Locale::Ko, _, true) => ("승인된 Desktop Pet 설치를 조정할까요?", "조정", "취소"),
        (Locale::En, _, true) => (
            "Reconcile approved Desktop Pet installation?",
            "Reconcile",
            "Cancel",
        ),
        (Locale::Ja, _, true) => (
            "承認済みの Desktop Pet インストールを照合しますか？",
            "照合",
            "キャンセル",
        ),
        (Locale::ZhHans, _, true) => ("核对已批准的 Desktop Pet 安装？", "核对", "取消"),
        (Locale::Ko, true, false) => (
            "Desktop Pet을 전용 업데이트 위치로 옮길까요?",
            "이동하고 다시 시작",
            "취소",
        ),
        (Locale::En, true, false) => (
            "Move Desktop Pet to its private update location?",
            "Move and restart",
            "Cancel",
        ),
        (Locale::Ja, true, false) => (
            "Desktop Pet を専用の更新場所へ移動しますか？",
            "移動して再起動",
            "キャンセル",
        ),
        (Locale::ZhHans, true, false) => {
            ("将 Desktop Pet 移至专用更新位置？", "移动并重启", "取消")
        }
        (Locale::Ko, false, false) => (
            "Desktop Pet만 업데이트할까요?",
            "Desktop Pet 업데이트",
            "취소",
        ),
        (Locale::En, false, false) => ("Update only Desktop Pet?", "Update Desktop Pet", "Cancel"),
        (Locale::Ja, false, false) => (
            "Desktop Pet だけを更新しますか？",
            "Desktop Pet を更新",
            "キャンセル",
        ),
        (Locale::ZhHans, false, false) => ("仅更新 Desktop Pet？", "更新 Desktop Pet", "取消"),
    }
}

fn migration_summary(plan: &MigrationPlan) -> String {
    let source_manager = match &plan.source_origin {
        desktop_pet_update_coordinator::migration::LegacyOrigin::Homebrew { brew, formula, cellar_formula_root, .. } =>
            format!("{} uninstall {formula} (legacy root {})", brew.path.display(), cellar_formula_root.display()),
        desktop_pet_update_coordinator::migration::LegacyOrigin::HerdrManaged { host, host_config_dir, plugin_root, integration_commit, .. } =>
            format!("{} managed desktop-pet removal (host config {}, plugin {}), then integration-only retirement at {integration_commit}",
                host.path.display(), host_config_dir.display(), plugin_root.display()),
        desktop_pet_update_coordinator::migration::LegacyOrigin::Generated { bundle, .. } =>
            format!("remove only verified generated bundle {} and captured aliases", bundle.display()),
        desktop_pet_update_coordinator::migration::LegacyOrigin::Manual { bundle } =>
            format!("remove only verified manual bundle {} and captured aliases", bundle.display()),
    };
    let mut summary = format!(
        "Operation {}\nHerdr source: {}\nVerified source root: {}\nDesktop Pet target: {} ({})\nTarget image: {}\nTarget source: {}\nExisting config/state profiles preserved; credentials remain in their original namespaces.\nDefault profile adoption: {}\nLegacy retirement approved: {} — {source_manager}\n",
        plan.operation_id, plan.source_image.path.display(), plan.source_origin.root().display(),
        plan.target.release.version, plan.target.release.tag, plan.target.image.path.display(),
        plan.target.origin.manager_label(), plan.adopt_as_default, plan.retirement_consent);
    match &plan.target.origin {
        InstallOrigin::Source { root } => {
            summary.push_str(&format!("Exact target source root: {}\n", root.display()))
        }
        InstallOrigin::Standalone { bundle } => {
            summary.push_str(&format!("Exact target bundle: {}\n", bundle.display()))
        }
        InstallOrigin::Homebrew {
            formula,
            cellar_formula_root,
            ..
        } => summary.push_str(&format!(
            "Exact target formula: {formula}\nTarget Cellar root: {}\n",
            cellar_formula_root.display()
        )),
        _ => {}
    }
    if let Some(manager) = &plan.target_manager_program {
        summary.push_str(&format!(
            "Verified target manager: {}\n",
            manager.path.display()
        ));
    }
    for alias in &plan.legacy_aliases {
        summary.push_str(&format!(
            "Verified legacy alias to retire: {}\n",
            alias.display()
        ));
    }
    for profile in &plan.profiles {
        summary.push_str(&format!(
            "\nProfile: {}\nState: {}\nInstance: {} · PID {}\nObservation: {:?}\n",
            profile.config_dir.display(),
            profile.state_dir.display(),
            profile.instance_id,
            profile.pid,
            profile.observation
        ));
    }
    if plan.tern_reconnect_required {
        summary.push_str(
            "\nTern reconnect confirmation is required separately after native readiness.\n",
        );
    }
    summary.push_str(
        "\nNo automatic manager replay or guaranteed rollback. Closing progress only hides it.",
    );
    summary
}

#[derive(Clone, Copy)]
enum Locale {
    En,
    Ko,
    ZhHans,
    Ja,
}

impl Locale {
    fn from_token(token: &str) -> Self {
        match token {
            "ko" => Self::Ko,
            "zh-Hans" => Self::ZhHans,
            "ja" => Self::Ja,
            _ => Self::En,
        }
    }

    fn title(self) -> String {
        let suffix = match self {
            Self::En => " app update",
            Self::Ko => " 앱 업데이트",
            Self::ZhHans => " 应用更新",
            Self::Ja => " アプリの更新",
        };
        format!("{}{suffix}", identity::DISPLAY_NAME)
    }

    fn details(self, expanded: bool) -> &'static str {
        match (self, expanded) {
            (Self::En, false) => "Show diagnostics",
            (Self::En, true) => "Hide diagnostics",
            (Self::Ko, false) => "진단 정보 보기",
            (Self::Ko, true) => "진단 정보 숨기기",
            (Self::ZhHans, false) => "显示诊断信息",
            (Self::ZhHans, true) => "隐藏诊断信息",
            (Self::Ja, false) => "診断情報を表示",
            (Self::Ja, true) => "診断情報を非表示",
        }
    }

    fn recovery(self) -> &'static str {
        match self {
            Self::En => "Recover verified start",
            Self::Ko => "확인 후 다시 시작",
            Self::ZhHans => "确认后重新启动",
            Self::Ja => "確認後に再起動",
        }
    }
}

fn source_label(origin: &InstallOrigin, locale: Locale) -> String {
    match origin {
        #[cfg(desktop_pet_legacy)]
        InstallOrigin::Herdr { requested_ref, .. } => {
            let reference = requested_ref.as_deref().unwrap_or(match locale {
                Locale::En | Locale::Ko => "(default)",
                Locale::ZhHans => "（默认）",
                Locale::Ja => "（既定）",
            });
            match locale {
                Locale::En | Locale::Ko => format!("Herdr ref {reference}"),
                Locale::ZhHans => format!("Herdr 引用 {reference}"),
                Locale::Ja => format!("Herdr 参照 {reference}"),
            }
        }
        InstallOrigin::Homebrew { formula, .. } => format!("Homebrew {formula}"),
        InstallOrigin::Source { .. } => match locale {
            Locale::En | Locale::Ko => "local source",
            Locale::ZhHans => "本地来源",
            Locale::Ja => "ローカルの入手元",
        }
        .into(),
        InstallOrigin::Standalone { .. } => match locale {
            Locale::En | Locale::Ko => "standalone application",
            Locale::ZhHans => "独立应用",
            Locale::Ja => "スタンドアロンアプリ",
        }
        .into(),
        InstallOrigin::Manual { .. } => match locale {
            Locale::En | Locale::Ko => "manual installation",
            Locale::ZhHans => "手动安装",
            Locale::Ja => "手動インストール",
        }
        .into(),
        InstallOrigin::Unknown { .. } => match locale {
            Locale::En | Locale::Ko => "unknown source",
            Locale::ZhHans => "未知来源",
            Locale::Ja => "不明な入手元",
        }
        .into(),
    }
}

fn release_summary(plan: &UpdatePlan, locale: Locale) -> String {
    let Some(target) = &plan.target else {
        return match locale {
            Locale::En => "Closing only hides this window; your profile is preserved.",
            Locale::Ko => "창을 닫아도 취소되지 않으며 프로필은 유지됩니다.",
            Locale::ZhHans => "关闭窗口只会隐藏此窗口；你的配置资料会保留。",
            Locale::Ja => "ウィンドウを閉じても非表示になるだけで、プロファイルは保持されます。",
        }
        .into();
    };
    let original = source_label(&plan.origin, locale);
    let destination = match &target.origin {
        #[cfg(desktop_pet_legacy)]
        InstallOrigin::Herdr { requested_ref, .. } => {
            let pinned = target
                .resolved_commit
                .as_deref()
                .or(requested_ref.as_deref())
                .unwrap_or(match locale {
                    Locale::En | Locale::Ko => "unknown",
                    Locale::ZhHans => "未知",
                    Locale::Ja => "不明",
                });
            let label = source_label(&target.origin, locale);
            match locale {
                Locale::En | Locale::Ko => {
                    format!("{label} ({}; pinned {pinned})", target.release.tag)
                }
                Locale::ZhHans => format!("{label}（{}；固定到 {pinned}）", target.release.tag),
                Locale::Ja => format!("{label}（{}、固定先 {pinned}）", target.release.tag),
            }
        }
        _ => source_label(&target.origin, locale),
    };
    match locale {
        Locale::En => format!(
            "{original} → {destination}\nRelease {} ({}) · existing profile preserved",
            target.release.version, target.release.tag
        ),
        Locale::Ko => format!(
            "{original} → {destination}\n출시 {} ({}) · 기존 프로필 유지",
            target.release.version, target.release.tag
        ),
        Locale::ZhHans => format!(
            "{original} → {destination}\n发行版本 {}（{}）· 保留现有配置资料",
            target.release.version, target.release.tag
        ),
        Locale::Ja => format!(
            "{original} → {destination}\nリリース {}（{}）· 既存のプロファイルを保持",
            target.release.version, target.release.tag
        ),
    }
}
#[cfg(target_os = "macos")]
mod mac {
    use super::UpdatePlan;
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
    // Keep exact paths and multi-profile details scrollable, without moving
    // affirmative/cancel controls beyond a small display's available height.
    unsafe fn set_consent_details(alert: Obj, summary: &str) {
        let init_view: unsafe extern "C" fn(Obj, Obj, Rect) -> Obj =
            std::mem::transmute(objc_msgSend as *const ());
        let frame = Rect {
            origin: Point { x: 0., y: 0. },
            size: Size {
                width: 520.,
                height: 320.,
            },
        };
        let details = init_view(
            object(class("NSTextView"), "alloc"),
            selector("initWithFrame:"),
            frame,
        );
        send_bool(details, "setEditable:", false);
        send_bool(details, "setSelectable:", true);
        send_bool(details, "setRichText:", false);
        send_object(
            details,
            "setTextColor:",
            object(class("NSColor"), "labelColor"),
        );
        send_bool(details, "setDrawsBackground:", false);
        send_bool(details, "setHorizontallyResizable:", false);
        send_bool(details, "setVerticallyResizable:", true);
        let set_size: unsafe extern "C" fn(Obj, Obj, Size) =
            std::mem::transmute(objc_msgSend as *const ());
        let maximum = Size {
            width: 520.,
            height: f64::MAX,
        };
        set_size(details, selector("setMaxSize:"), maximum);
        let container = object(details, "textContainer");
        set_size(container, selector("setContainerSize:"), maximum);
        send_bool(container, "setWidthTracksTextView:", true);
        send_object(details, "setString:", string(summary));
        let scroll = init_view(
            object(class("NSScrollView"), "alloc"),
            selector("initWithFrame:"),
            frame,
        );
        send_object(scroll, "setDocumentView:", details);
        send_bool(scroll, "setHasVerticalScroller:", true);
        send_bool(scroll, "setAutohidesScrollers:", true);
        send_bool(scroll, "setDrawsBackground:", false);
        send_object(alert, "setAccessoryView:", scroll);
        send_void(details, "release");
        send_void(scroll, "release");
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
            let name = CString::new("DesktopPetUpdateCoordinatorRecoveryTarget").unwrap();
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
        fn text(self, locale: super::Locale) -> (&'static str, &'static str) {
            use super::Locale;
            match (self, locale) {
                (Self::Preparing, Locale::En) => (
                    "Preparing update",
                    "Checking and staging the release while the original app stays running.",
                ),
                (Self::Preparing, Locale::Ko) => (
                    "업데이트 준비 중",
                    "기존 앱을 실행한 채 릴리스를 확인하고 준비합니다.",
                ),
                (Self::Preparing, Locale::ZhHans) => {
                    ("正在准备更新", "保持原应用运行，同时检查并准备发行版本。")
                }
                (Self::Preparing, Locale::Ja) => (
                    "更新を準備中",
                    "元のアプリを起動したまま、リリースを確認して準備します。",
                ),
                (Self::Stopping, Locale::En) => {
                    ("Stopping app", "Only the prepared app instance is stopped.")
                }
                (Self::Stopping, Locale::Ko) => ("앱 종료 중", "준비된 앱 인스턴스만 종료합니다."),
                (Self::Stopping, Locale::ZhHans) => ("正在停止应用", "仅停止已准备好的应用实例。"),
                (Self::Stopping, Locale::Ja) => (
                    "アプリを終了中",
                    "準備済みのアプリのインスタンスのみを終了します。",
                ),
                (Self::Installing, Locale::En) => (
                    "Installing update",
                    "Installing the approved target version and source.",
                ),
                (Self::Installing, Locale::Ko) => (
                    "업데이트 설치 중",
                    "승인한 대상 버전과 설치 원본으로 설치합니다.",
                ),
                (Self::Installing, Locale::ZhHans) => {
                    ("正在安装更新", "按已批准的目标版本和安装来源进行安装。")
                }
                (Self::Installing, Locale::Ja) => (
                    "更新をインストール中",
                    "承認された対象バージョンと入手元からインストールします。",
                ),
                (Self::Validating, Locale::En) => (
                    "Verifying installation",
                    "Checking the installed app and its source.",
                ),
                (Self::Validating, Locale::Ko) => {
                    ("설치 확인 중", "실제 설치된 앱과 원본을 확인합니다.")
                }
                (Self::Validating, Locale::ZhHans) => {
                    ("正在验证安装", "检查实际安装的应用及其来源。")
                }
                (Self::Validating, Locale::Ja) => (
                    "インストールを検証中",
                    "実際にインストールされたアプリと入手元を確認します。",
                ),
                (Self::Starting, Locale::En) => (
                    "Starting updated app",
                    "Checking readiness on your existing profile.",
                ),
                (Self::Starting, Locale::Ko) => (
                    "업데이트된 앱 시작 중",
                    "기존 프로필로 시작하고 준비 상태를 확인합니다.",
                ),
                (Self::Starting, Locale::ZhHans) => (
                    "正在启动更新后的应用",
                    "使用现有配置资料启动，并确认应用已就绪。",
                ),
                (Self::Starting, Locale::Ja) => (
                    "更新後のアプリを起動中",
                    "既存のプロファイルで起動し、準備完了を確認します。",
                ),
                (Self::RestoringOriginal, Locale::En) => (
                    "Starting original app",
                    "Checking the original version on your profile; this is not an upgrade.",
                ),
                (Self::RestoringOriginal, Locale::Ko) => (
                    "원래 앱 시작 중",
                    "기존 프로필로 원래 버전을 확인합니다. 업데이트는 아닙니다.",
                ),
                (Self::RestoringOriginal, Locale::ZhHans) => {
                    ("正在启动原应用", "使用现有配置资料检查原版本；这不是升级。")
                }
                (Self::RestoringOriginal, Locale::Ja) => (
                    "元のアプリを起動中",
                    "既存のプロファイルで元のバージョンを確認します。更新ではありません。",
                ),
                (Self::Reconciling, Locale::En) => (
                    "Checking update state",
                    "Checking safely without repeating installation.",
                ),
                (Self::Reconciling, Locale::Ko) => (
                    "업데이트 상태 확인 중",
                    "설치를 반복하지 않고 안전한 상태를 확인합니다.",
                ),
                (Self::Reconciling, Locale::ZhHans) => {
                    ("正在检查更新状态", "不重复安装，安全地检查当前状态。")
                }
                (Self::Reconciling, Locale::Ja) => (
                    "更新状態を確認中",
                    "インストールを繰り返さず、安全に状態を確認します。",
                ),
                (Self::Finalizing, Locale::En) => {
                    ("Finalizing", "App readiness verified. Saving the result.")
                }
                (Self::Finalizing, Locale::Ko) => (
                    "마무리 중",
                    "앱 준비 상태를 확인했습니다. 기록을 저장합니다.",
                ),
                (Self::Finalizing, Locale::ZhHans) => {
                    ("正在完成", "已确认应用就绪。正在保存结果。")
                }
                (Self::Finalizing, Locale::Ja) => (
                    "完了処理中",
                    "アプリの準備完了を確認しました。結果を保存します。",
                ),
                (Self::Attention(Attention::NotStarted), Locale::En) => (
                    "Update could not start",
                    "Review details or check the update status.",
                ),
                (Self::Attention(Attention::NotStarted), Locale::Ko) => (
                    "업데이트를 시작하지 못했습니다",
                    "상세 내용을 확인하거나 업데이트 상태를 조회하세요.",
                ),
                (Self::Attention(Attention::NotStarted), Locale::ZhHans) => {
                    ("无法开始更新", "请查看详细信息或检查更新状态。")
                }
                (Self::Attention(Attention::NotStarted), Locale::Ja) => (
                    "更新を開始できませんでした",
                    "詳細を確認するか、更新状態を調べてください。",
                ),
                (Self::Attention(Attention::Ownership), Locale::En) => (
                    "Updater handoff failed",
                    "The original app was not stopped. Review details.",
                ),
                (Self::Attention(Attention::Ownership), Locale::Ko) => (
                    "업데이트 창 인계 실패",
                    "기존 앱은 종료하지 않았습니다. 상세 내용을 확인하세요.",
                ),
                (Self::Attention(Attention::Ownership), Locale::ZhHans) => {
                    ("更新程序交接失败", "原应用未停止。请查看详细信息。")
                }
                (Self::Attention(Attention::Ownership), Locale::Ja) => (
                    "更新処理の引き継ぎに失敗",
                    "元のアプリは終了していません。詳細を確認してください。",
                ),
                (Self::Attention(Attention::Completion), Locale::En) => (
                    "Could not save completion",
                    "The new app is ready, but completion needs review.",
                ),
                (Self::Attention(Attention::Completion), Locale::Ko) => (
                    "완료 기록을 저장하지 못했습니다",
                    "새 앱은 준비됐지만 완료 확인이 필요합니다.",
                ),
                (Self::Attention(Attention::Completion), Locale::ZhHans) => {
                    ("无法保存完成记录", "新应用已就绪，但仍需检查完成状态。")
                }
                (Self::Attention(Attention::Completion), Locale::Ja) => (
                    "完了記録を保存できませんでした",
                    "新しいアプリは準備できていますが、完了状態の確認が必要です。",
                ),
                (Self::Attention(Attention::Release), Locale::En) => (
                    "Could not release reservation",
                    "The new app is ready, but recovery needs review.",
                ),
                (Self::Attention(Attention::Release), Locale::Ko) => (
                    "예약 해제 실패",
                    "새 앱은 준비됐지만 복구 확인이 필요합니다.",
                ),
                (Self::Attention(Attention::Release), Locale::ZhHans) => {
                    ("无法解除保留状态", "新应用已就绪，但仍需检查恢复状态。")
                }
                (Self::Attention(Attention::Release), Locale::Ja) => (
                    "予約を解除できませんでした",
                    "新しいアプリは準備できていますが、復旧状態の確認が必要です。",
                ),
                (Self::Attention(Attention::Interrupted), Locale::En) => (
                    "Update needs attention",
                    "No automatic retry or rollback. Review details.",
                ),
                (Self::Attention(Attention::Interrupted), Locale::Ko) => (
                    "업데이트 확인 필요",
                    "자동 재시도나 되돌리기는 하지 않습니다. 상세 내용을 확인하세요.",
                ),
                (Self::Attention(Attention::Interrupted), Locale::ZhHans) => {
                    ("更新需要检查", "不会自动重试或回滚。请查看详细信息。")
                }
                (Self::Attention(Attention::Interrupted), Locale::Ja) => (
                    "更新の確認が必要です",
                    "自動で再試行やロールバックはしません。詳細を確認してください。",
                ),
                (Self::Attention(Attention::Recovery), Locale::En) => (
                    "Recovery needs attention",
                    "The reservation is retained. Review details.",
                ),
                (Self::Attention(Attention::Recovery), Locale::Ko) => (
                    "복구 확인 필요",
                    "예약을 유지했습니다. 상세 내용을 확인하세요.",
                ),
                (Self::Attention(Attention::Recovery), Locale::ZhHans) => {
                    ("恢复需要检查", "保留状态仍在。请查看详细信息。")
                }
                (Self::Attention(Attention::Recovery), Locale::Ja) => (
                    "復旧の確認が必要です",
                    "予約は保持されています。詳細を確認してください。",
                ),
                (Self::Attention(Attention::OriginalUnchanged), Locale::En) => (
                    "Update was not applied",
                    "The original app remains or was restored; this is not an upgrade.",
                ),
                (Self::Attention(Attention::OriginalUnchanged), Locale::Ko) => (
                    "업데이트가 적용되지 않았습니다",
                    "원래 앱이 유지되거나 복원됐습니다. 새 버전은 적용되지 않았습니다.",
                ),
                (Self::Attention(Attention::OriginalUnchanged), Locale::ZhHans) => {
                    ("更新未应用", "原应用保持不变或已恢复；新版本未应用。")
                }
                (Self::Attention(Attention::OriginalUnchanged), Locale::Ja) => (
                    "更新は適用されませんでした",
                    "元のアプリが維持または復元されました。新しいバージョンは適用されていません。",
                ),
                (Self::UserStopped, Locale::En) => (
                    "App stopped by user",
                    "The app will not be restarted automatically.",
                ),
                (Self::UserStopped, Locale::Ko) => (
                    "사용자가 앱을 종료했습니다",
                    "앱을 자동으로 다시 시작하지 않습니다.",
                ),
                (Self::UserStopped, Locale::ZhHans) => ("用户已停止应用", "不会自动重新启动应用。"),
                (Self::UserStopped, Locale::Ja) => (
                    "ユーザーがアプリを終了しました",
                    "アプリは自動的に再起動しません。",
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
        locale: super::Locale,
        expanded: std::cell::Cell<bool>,
        migration_authorization: Option<[u8; 32]>,
        migration_description: Option<String>,
        install_authorization: Option<[u8; 32]>,
    }
    impl Panel {
        pub fn new(locale: &str, plan: &UpdatePlan) -> Result<Self, String> {
            desktop_pet_update_coordinator::protocol::validate_operation_id(&plan.operation_id)?;
            Self::new_summary(
                locale,
                &super::release_summary(plan, super::Locale::from_token(locale)),
                None,
            )
        }
        pub fn for_migration(locale: &str, plan: &super::MigrationPlan) -> Result<Self, String> {
            desktop_pet_update_coordinator::protocol::validate_operation_id(&plan.operation_id)?;
            let footer = format!(
                "Herdr → Desktop Pet {}\n{} profiles preserved · legacy retirement: {}",
                plan.target.release.version,
                plan.profiles.len(),
                plan.retirement_consent
            );
            let mut panel =
                Self::new_summary(locale, &footer, Some(super::migration_digest(plan)?))?;
            panel.migration_description = Some(super::migration_summary(plan));
            panel.show(Phase::Preparing, panel.migration_description.as_deref());
            Ok(panel)
        }
        pub fn for_install(
            locale: &str,
            plan: &super::StandaloneInstallPlan,
        ) -> Result<Self, String> {
            plan.validate()?;
            let summary = super::install_summary(plan, super::Locale::from_token(locale), false)?;
            let mut panel = Self::new_summary(locale, &summary, None)?;
            panel.install_authorization = Some(super::install_digest(plan)?);
            Ok(panel)
        }
        pub fn confirm_install(
            &self,
            plan: &desktop_pet_update_coordinator::standalone_install::StandaloneInstallPlan,
            recovery: bool,
        ) -> Result<bool, String> {
            plan.validate()?;
            self.require_install_visible(plan)?;
            self.activate();
            unsafe {
                let _pool = AutoreleasePool::new();
                let alert = object(object(class("NSAlert"), "alloc"), "init");
                if alert.is_null() {
                    return Err("native install consent alert unavailable".into());
                }
                let (title, affirmative, cancel) =
                    super::install_buttons(self.locale, !plan.purpose.is_upgrade(), recovery);
                send_object(alert, "setMessageText:", string(title));
                set_consent_details(alert, &super::install_summary(plan, self.locale, recovery)?);
                send_object(alert, "addButtonWithTitle:", string(affirmative));
                send_object(alert, "addButtonWithTitle:", string(cancel));
                let modal: unsafe extern "C" fn(Obj, Obj) -> isize =
                    std::mem::transmute(objc_msgSend as *const ());
                let response = modal(alert, selector("runModal"));
                send_void(alert, "release");
                self.pump();
                super::install_confirmation_response(
                    self.install_authorization,
                    super::install_digest(plan)?,
                    self.visible(),
                    response,
                )
            }
        }
        pub fn require_install_visible(
            &self,
            plan: &super::StandaloneInstallPlan,
        ) -> Result<(), String> {
            self.pump();
            super::require_install_authorization(
                self.install_authorization,
                super::install_digest(plan)?,
                self.visible(),
            )
        }
        fn new_summary(
            locale: &str,
            summary: &str,
            migration_authorization: Option<[u8; 32]>,
        ) -> Result<Self, String> {
            if unsafe { libc::pthread_main_np() } != 1 {
                return Err("native updater UI must run on the main thread".into());
            }
            unsafe {
                let _pool = AutoreleasePool::new();
                let locale = super::Locale::from_token(locale);
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
                            width: 700.,
                            height: 230.,
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
                send_object(window, "setTitle:", string(&locale.title()));
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
                                width: 655.,
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
                let heading = field(179., 27.);
                let explanation = field(149., 25.);
                let footer = field(52., 68.);
                send_object(footer, "setStringValue:", string(summary));
                let progress = init_view(
                    object(class("NSProgressIndicator"), "alloc"),
                    selector("initWithFrame:"),
                    Rect {
                        origin: Point { x: 22., y: 128. },
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
                let details_button =
                    button(22., 175., "toggleUpdateDetails:", locale.details(false));
                let recovery_button = button(476., 200., "recoverUpdate:", locale.recovery());
                let diagnostics = init_view(
                    object(class("NSScrollView"), "alloc"),
                    selector("initWithFrame:"),
                    Rect {
                        origin: Point { x: 22., y: 149. },
                        size: Size {
                            width: 655.,
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
                            width: 655.,
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
                    locale,
                    expanded: std::cell::Cell::new(false),
                    migration_authorization,
                    migration_description: None,
                    install_authorization: None,
                };
                panel.show(Phase::Preparing, None);
                send_void(window, "center");
                send_void(window, "orderFrontRegardless");
                panel.pump();
                Ok(panel)
            }
        }
        pub fn migration_diagnostics(&self, detail: Option<&str>) -> Option<String> {
            self.migration_description
                .as_ref()
                .map(|description| match detail {
                    Some(detail) => format!("{detail}\n\n{description}"),
                    None => description.clone(),
                })
        }
        pub fn confirm_migration(
            &self,
            plan: &super::MigrationPlan,
            retirement_only: bool,
        ) -> Result<bool, String> {
            self.require_migration_visible(plan)?;
            unsafe {
                let _pool = AutoreleasePool::new();
                let alert = object(object(class("NSAlert"), "alloc"), "init");
                if alert.is_null() {
                    return Err("native migration consent alert unavailable".into());
                }
                send_object(
                    alert,
                    "setMessageText:",
                    string(if retirement_only {
                        "Resume approved legacy retirement?"
                    } else {
                        "Migrate Herdr to Desktop Pet?"
                    }),
                );
                set_consent_details(alert, &super::migration_summary(plan));
                send_object(
                    alert,
                    "addButtonWithTitle:",
                    string(if retirement_only {
                        "Resume retirement"
                    } else {
                        "Migrate"
                    }),
                );
                send_object(alert, "addButtonWithTitle:", string("Cancel"));
                let modal: unsafe extern "C" fn(Obj, Obj) -> isize =
                    std::mem::transmute(objc_msgSend as *const ());
                let response = modal(alert, selector("runModal"));
                send_void(alert, "release");
                self.pump();
                super::migration_confirmation_response(response)
            }
        }
        pub fn require_migration_visible(&self, plan: &super::MigrationPlan) -> Result<(), String> {
            if self.migration_authorization != Some(super::migration_digest(plan)?) {
                return Err(
                    "migration window is not bound to this source/target/profile authorization"
                        .into(),
                );
            }
            self.pump();
            if !self.visible() {
                self.activate();
            }
            if !self.visible() {
                return Err("independent native migration window is not visible".into());
            }
            Ok(())
        }
        pub fn set_status(&self, heading: &str, explanation: &str) {
            unsafe {
                let _pool = AutoreleasePool::new();
                send_object(self.heading, "setStringValue:", string(heading));
                send_object(self.explanation, "setStringValue:", string(explanation));
            }
            self.pump();
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
                let (heading, explanation) = phase.text(self.locale);
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
                        width: 700.,
                        height: if expanded { 425. } else { 230. },
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
                        y: 179. + offset,
                    },
                );
                move_view(
                    self.explanation,
                    selector("setFrameOrigin:"),
                    Point {
                        x: 22.,
                        y: 149. + offset,
                    },
                );
                send_bool(self.diagnostics, "setHidden:", !expanded);
                send_object(
                    self.details_button,
                    "setTitle:",
                    string(self.locale.details(expanded)),
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
    pub fn new(_: &str, _: &UpdatePlan) -> Result<Self, String> {
        Err("native updater requires macOS AppKit".into())
    }
    pub fn for_migration(_: &str, _: &MigrationPlan) -> Result<Self, String> {
        Err("native migration requires macOS AppKit".into())
    }
    pub fn for_install(
        _: &str,
        _: &desktop_pet_update_coordinator::standalone_install::StandaloneInstallPlan,
    ) -> Result<Self, String> {
        Err("native install requires macOS AppKit".into())
    }
    pub fn confirm_install(
        &self,
        _: &desktop_pet_update_coordinator::standalone_install::StandaloneInstallPlan,
        _: bool,
    ) -> Result<bool, String> {
        Err("native install requires macOS AppKit".into())
    }
    pub fn require_install_visible(&self, _: &StandaloneInstallPlan) -> Result<(), String> {
        Err("native install requires macOS AppKit".into())
    }
    pub fn confirm_migration(&self, _: &MigrationPlan, _: bool) -> Result<bool, String> {
        Err("native migration requires macOS AppKit".into())
    }
    pub fn require_migration_visible(&self, _: &MigrationPlan) -> Result<(), String> {
        Err("native migration requires macOS AppKit".into())
    }
    pub fn set_status(&self, _: &str, _: &str) {}
    pub fn migration_diagnostics(&self, _: Option<&str>) -> Option<String> {
        None
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

impl MigrationPresentation for Panel {
    fn show(&self, phase: MigrationPhase, detail: Option<&str>) {
        let (progress, heading, explanation) = match phase {
            MigrationPhase::Validating => (
                Phase::Validating,
                "Verifying migration",
                "Checking exact source, target and profiles.",
            ),
            MigrationPhase::Staged => (
                Phase::Preparing,
                "Migration staged",
                "Original app remains running.",
            ),
            MigrationPhase::Preparing => (
                Phase::Preparing,
                "Preparing migration",
                "Preparing only authorized source instances.",
            ),
            MigrationPhase::Stopping => (
                Phase::Stopping,
                "Stopping legacy app",
                "Stopping only prepared source profiles.",
            ),
            MigrationPhase::Adopting => (
                Phase::Installing,
                "Adopting Desktop Pet",
                "Preserving captured config/state profiles.",
            ),
            MigrationPhase::Starting => (
                Phase::Starting,
                "Starting Desktop Pet",
                "Checking native UI/control and exact profile readiness.",
            ),
            MigrationPhase::NativeReady => (
                Phase::Finalizing,
                "Desktop Pet native-ready",
                "Legacy retirement and reconnect are not yet complete.",
            ),
            MigrationPhase::Integrating => (
                Phase::Finalizing,
                "Connecting approved integrations",
                "App committed. Waiting automatically for the loaded consumer and actual same-profile SDK binding before legacy retirement.",
            ),
            MigrationPhase::Retiring => (
                Phase::Installing,
                "Retiring legacy installation",
                "Running only the explicitly authorized legacy removal.",
            ),
            MigrationPhase::RetirementFailed => (
                Phase::Attention(Attention::Interrupted),
                "Migration incomplete; retirement pending",
                "App state and integration errors are retained. The legacy owner is not removed before every required connection is verified.",
            ),
            MigrationPhase::ManagerUnknown => (
                Phase::Attention(Attention::Interrupted),
                "Manager outcome unknown",
                "No automatic replay; inspect the retained journal.",
            ),
            MigrationPhase::AwaitingReconnect => (
                Phase::Attention(Attention::Completion),
                "App cutover recorded; integration status is separate",
                "Current-owner verification can settle this retained record without manager replay.",
            ),
            MigrationPhase::Complete => (
                Phase::Finalizing,
                "Migration complete",
                "Every affected profile committed and required integrations connected before approved legacy retirement.",
            ),
            MigrationPhase::RollingBack => (
                Phase::RestoringOriginal,
                "Restoring original profile",
                "Checking the exact captured source; no upgrade claimed.",
            ),
            MigrationPhase::RolledBack => (
                Phase::Attention(Attention::OriginalUnchanged),
                "Migration not applied",
                "Original profile restored; Desktop Pet migration is not complete.",
            ),
            MigrationPhase::UserStopped => (
                Phase::UserStopped,
                "App stopped by user",
                "No updater restart or manager replay.",
            ),
            MigrationPhase::Failed => (
                Phase::Attention(Attention::Interrupted),
                "Migration needs attention",
                "Review diagnostics; no automatic retry.",
            ),
        };
        let diagnostic = self.migration_diagnostics(detail);
        Panel::show(self, progress, diagnostic.as_deref().or(detail));
        self.set_status(heading, explanation);
    }
    fn pump(&self) {
        Panel::pump(self);
    }
    fn require_visible(&self, plan: &MigrationPlan) -> Result<(), String> {
        self.require_migration_visible(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        install_confirmation_response, migration_confirmation_response,
        require_install_authorization,
    };

    #[test]
    fn native_final_consent_only_accepts_real_approve_or_cancel_responses() {
        assert_eq!(migration_confirmation_response(1000).unwrap(), true);
        assert_eq!(migration_confirmation_response(1001).unwrap(), false);
        for response in [-1002, -1001, 0, 1, 1002, isize::MIN, isize::MAX] {
            assert!(migration_confirmation_response(response).is_err());
        }
    }

    #[test]
    fn install_consent_requires_exact_approval_and_visible_window() {
        let digest = [7; 32];
        assert!(require_install_authorization(Some(digest), digest, true).is_ok());
        assert!(require_install_authorization(None, digest, true).is_err());
        assert!(require_install_authorization(Some([8; 32]), digest, true).is_err());
        assert!(require_install_authorization(Some(digest), digest, false).is_err());
        assert!(install_confirmation_response(Some(digest), digest, true, 1000).unwrap());
        assert!(!install_confirmation_response(Some(digest), digest, true, 1001).unwrap());
        assert!(install_confirmation_response(Some(digest), digest, false, 1000).is_err());
        assert!(install_confirmation_response(Some([8; 32]), digest, true, 1000).is_err());
        for response in [-1002, -1001, 0, 1, 1002, isize::MIN, isize::MAX] {
            assert!(install_confirmation_response(Some(digest), digest, true, response).is_err());
        }
    }
}

#[cfg(test)]
mod install_authorization_tests {
    use super::*;
    use desktop_pet_update_coordinator::installation::{InstallationKind, InstallationRecord};
    use desktop_pet_update_coordinator::native_runtime::ProfilePaths;
    use desktop_pet_update_coordinator::standalone_install::{
        InstallSource, StandaloneInstallPurpose,
    };
    use desktop_pet_update_coordinator::standalone_relocation::SettledRelocationParent;
    use desktop_pet_update_coordinator::ExecutableIdentity;
    use std::path::PathBuf;

    #[test]
    fn displayed_approval_digest_binds_source_target_profile_and_purpose() {
        let source = PathBuf::from("/approval-fixture/Applications/DesktopPet.app");
        let image = ExecutableIdentity {
            path: source.join("Contents/MacOS/desktop-pet"),
            sha256: "a".repeat(64),
        };
        let profile = ProfilePaths {
            config_dir: "/approval-fixture/config".into(),
            state_dir: "/approval-fixture/state".into(),
        };
        let parent = SettledRelocationParent {
            version: 1,
            operation_id: "2".repeat(32),
            plan_sha256: "b".repeat(64),
            generation: 7,
            candidate_image: image.clone(),
            source_closure_sha256: "c".repeat(64),
            profile: profile.clone(),
            result_sha256: "d".repeat(64),
        };
        let mut plan = StandaloneInstallPlan {
            version: 2,
            operation_id: "1".repeat(32),
            purpose: StandaloneInstallPurpose::PrivateRelocation {
                target: InstallOrigin::Standalone {
                    bundle: "/approval-fixture/private/DesktopPet.app".into(),
                },
                parent,
                source_closure_sha256: "c".repeat(64),
                tern: None,
            },
            original: InstallationRecord {
                version: 1,
                executable: image.path.clone(),
                kind: InstallationKind::Standalone,
                app_version: "0.4.13".into(),
                protocol: 4,
                observation_mode: 2,
                integration_api: 1,
                origin: InstallOrigin::Standalone { bundle: source },
                image: image.clone(),
            },
            candidate_bundle: "/approval-fixture/stage/DesktopPet.app".into(),
            candidate_image: image.clone(),
            candidate_version: "0.4.13".into(),
            helper_image: image,
            source: InstallSource::Stopped {
                profile: profile.clone(),
            },
            profile,
            original_stop_generation: 7,
            control_token: "e".repeat(64),
        };
        let displayed = install_digest(&plan).unwrap();
        let original = plan.clone();
        plan.profile.state_dir = "/approval-fixture/other-state".into();
        assert!(require_install_authorization(
            Some(displayed),
            install_digest(&plan).unwrap(),
            true
        )
        .is_err());
        plan = original.clone();
        plan.original.image.sha256 = "f".repeat(64);
        assert!(require_install_authorization(
            Some(displayed),
            install_digest(&plan).unwrap(),
            true
        )
        .is_err());
        plan = original.clone();
        if let StandaloneInstallPurpose::PrivateRelocation { target, .. } = &mut plan.purpose {
            *target = InstallOrigin::Standalone {
                bundle: "/approval-fixture/other-target/DesktopPet.app".into(),
            };
        }
        assert!(require_install_authorization(
            Some(displayed),
            install_digest(&plan).unwrap(),
            true
        )
        .is_err());
        plan = original;
        plan.purpose = StandaloneInstallPurpose::Upgrade;
        assert!(require_install_authorization(
            Some(displayed),
            install_digest(&plan).unwrap(),
            true
        )
        .is_err());
    }
}
