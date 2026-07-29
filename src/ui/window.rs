use gtk4::prelude::*;
use gtk4::{
    Adjustment, ApplicationWindow, Box, Button, Label, Orientation, Switch,
};
use libadwaita::prelude::*;
use libadwaita::{ActionRow, ComboRow, HeaderBar, PreferencesGroup, SpinRow};

use crate::system::permissions::PermissionStatus;

pub struct MainWindow {
    pub window: ApplicationWindow,
    pub cps_spin: SpinRow,
    pub cps_value_label: Label,
    pub preset_10: Button,
    pub preset_20: Button,
    pub preset_50: Button,
    pub preset_100: Button,
    pub start_stop_btn: Button,
    pub status_label: Label,
    pub status_icon: Label,
    pub live_cps_label: Label,
    pub mode_row: ComboRow,
    pub burst_row: SpinRow,
    pub button_row: ComboRow,
    pub jitter_switch: Switch,
    pub jitter_min_row: SpinRow,
    pub jitter_max_row: SpinRow,
    pub hotkey_btn: Button,
    pub hotkey_label: Label,
    pub toast_overlay: libadwaita::ToastOverlay,
}

impl MainWindow {
    pub fn new(app: &libadwaita::Application, perms: &PermissionStatus) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Clix")
            .default_width(420)
            .default_height(580)
            .resizable(false)
            .build();

        let header = HeaderBar::builder()
            .title_widget(&libadwaita::WindowTitle::new("Clix", "Auto Clicker"))
            .build();
        window.set_titlebar(Some(&header));

        let content = Box::new(Orientation::Vertical, 8);
        content.set_margin_top(8);
        content.set_margin_bottom(8);
        content.set_margin_start(12);
        content.set_margin_end(12);

        if !perms.uinput_accessible {
            let permission_banner = Label::new(Some(&perms.message));
            permission_banner.add_css_class("error");
            permission_banner.set_margin_top(12);
            permission_banner.set_margin_bottom(6);
            permission_banner.set_margin_start(12);
            permission_banner.set_margin_end(12);
            permission_banner.set_wrap(true);
            content.append(&permission_banner);
        }

        // ── Status Card (Compact) ──
        let status_box = Box::new(Orientation::Vertical, 4);
        status_box.add_css_class("card");
        status_box.set_margin_top(4);
        status_box.set_margin_bottom(4);

        let status_header = Box::new(Orientation::Horizontal, 10);
        status_header.set_margin_top(12);
        status_header.set_margin_bottom(4);
        status_header.set_margin_start(12);
        status_header.set_margin_end(12);

        let status_icon_label = Label::new(Some("●"));
        status_icon_label.add_css_class("title-2");
        status_icon_label.add_css_class("error");

        let status_label = Label::new(Some("Stopped"));
        status_label.add_css_class("title-3");
        status_label.set_halign(gtk4::Align::Start);
        status_label.set_hexpand(true);

        status_header.append(&status_icon_label);
        status_header.append(&status_label);

        let live_cps_label = Label::new(Some("Live: 0.0 CPS"));
        live_cps_label.add_css_class("title-4");
        live_cps_label.set_halign(gtk4::Align::Center);
        live_cps_label.set_margin_bottom(12);

        status_box.append(&status_header);
        status_box.append(&live_cps_label);

        // ── Main Settings Groups ──
        let settings_group = PreferencesGroup::new();
        settings_group.set_margin_top(4);

        // CPS row
        let cps_adj = Adjustment::new(20.0, 1.0, 1000.0, 1.0, 10.0, 0.0);
        let cps_spin = SpinRow::new(Some(&cps_adj), 1.0, 0);
        cps_spin.set_title("Clicks Per Second");

        let cps_value_label = Label::new(Some("20 CPS"));
        cps_value_label.add_css_class("dim-label");

        // CPS Quick Presets
        let preset_box = Box::new(Orientation::Horizontal, 6);
        preset_box.set_margin_start(12);
        preset_box.set_margin_end(12);
        preset_box.set_margin_top(4);
        preset_box.set_margin_bottom(8);
        preset_box.set_halign(gtk4::Align::Center);

        let preset_label = Label::new(Some("Quick:"));
        preset_label.add_css_class("dim-label");
        preset_box.append(&preset_label);

        let preset_10 = Button::with_label("10");
        preset_10.add_css_class("flat");
        preset_box.append(&preset_10);

        let preset_20 = Button::with_label("20");
        preset_20.add_css_class("flat");
        preset_box.append(&preset_20);

        let preset_50 = Button::with_label("50");
        preset_50.add_css_class("flat");
        preset_box.append(&preset_50);

        let preset_100 = Button::with_label("100");
        preset_100.add_css_class("flat");
        preset_box.append(&preset_100);

        // Mode row with tooltip
        let mode_row = ComboRow::new();
        mode_row.set_title("Mode");
        mode_row.set_tooltip_text(Some("Toggle: Start/stop with hotkey | Hold: Click while holding hotkey | Burst: Fire fixed number of clicks"));
        let mode_model = gtk4::StringList::new(&["Toggle", "Hold", "Burst"]);
        mode_row.set_model(Some(&mode_model));

        // Burst count
        let burst_adj = Adjustment::new(10.0, 1.0, 10_000.0, 1.0, 10.0, 0.0);
        let burst_row = SpinRow::new(Some(&burst_adj), 1.0, 0);
        burst_row.set_title("Burst Count");
        burst_row.set_visible(false);

        // Mouse button
        let button_row = ComboRow::new();
        button_row.set_title("Mouse Button");
        let button_model = gtk4::StringList::new(&["Left Click", "Right Click"]);
        button_row.set_model(Some(&button_model));

        // Hotkey - button that captures next key press
        let hotkey_label = Label::new(Some("F6"));
        hotkey_label.set_halign(gtk4::Align::End);
        hotkey_label.add_css_class("dim-label");

        let hotkey_btn = Button::new();
        hotkey_btn.set_label("Set Hotkey");
        hotkey_btn.add_css_class("flat");
        hotkey_btn.set_halign(gtk4::Align::End);

        let hotkey_box = Box::new(Orientation::Horizontal, 4);
        hotkey_box.append(&hotkey_label);
        hotkey_box.append(&hotkey_btn);

        let hotkey_row = ActionRow::new();
        hotkey_row.set_title("Hotkey");
        hotkey_row.add_suffix(&hotkey_box);

        // Jitter switch row
        let jitter_switch = Switch::new();
        jitter_switch.set_active(false);
        jitter_switch.set_valign(gtk4::Align::Center);

        let jitter_row_box = libadwaita::ActionRow::new();
        jitter_row_box.set_title("Random Jitter");
        jitter_row_box.add_suffix(&jitter_switch);
        jitter_row_box.set_activatable_widget(Some(&jitter_switch));

        // Jitter timing (compact - inline)
        let jitter_timing_box = Box::new(Orientation::Horizontal, 8);
        jitter_timing_box.set_margin_start(12);
        jitter_timing_box.set_margin_end(12);
        jitter_timing_box.set_margin_top(4);
        jitter_timing_box.set_margin_bottom(4);

        let jitter_min_adj = Adjustment::new(5.0, 1.0, 100.0, 1.0, 5.0, 0.0);
        let jitter_min_row = SpinRow::new(Some(&jitter_min_adj), 1.0, 0);
        jitter_min_row.set_title("Min (ms)");
        jitter_min_row.set_hexpand(true);

        let jitter_max_adj = Adjustment::new(20.0, 1.0, 200.0, 1.0, 5.0, 0.0);
        let jitter_max_row = SpinRow::new(Some(&jitter_max_adj), 1.0, 0);
        jitter_max_row.set_title("Max (ms)");
        jitter_max_row.set_hexpand(true);

        settings_group.add(&cps_spin);
        settings_group.add(&mode_row);
        settings_group.add(&burst_row);
        settings_group.add(&button_row);
        settings_group.add(&hotkey_row);
        settings_group.add(&jitter_row_box);

        let jitter_detail_group = PreferencesGroup::new();
        jitter_detail_group.add(&jitter_min_row);
        jitter_detail_group.add(&jitter_max_row);

        // ── Control Button ──
        let start_stop_btn = Button::with_label("Start Clicking");
        start_stop_btn.add_css_class("suggested-action");
        start_stop_btn.add_css_class("pill");
        start_stop_btn.set_margin_top(8);
        start_stop_btn.set_margin_bottom(4);
        start_stop_btn.set_height_request(44);

        content.append(&status_box);
        content.append(&preset_box);
        content.append(&settings_group);
        content.append(&jitter_detail_group);
        content.append(&start_stop_btn);

        // Wrap content in toast overlay for notifications
        let toast_overlay = libadwaita::ToastOverlay::new();
        toast_overlay.set_child(Some(&content));

        window.set_child(Some(&toast_overlay));

        Self {
            window,
            cps_spin,
            cps_value_label,
            preset_10,
            preset_20,
            preset_50,
            preset_100,
            start_stop_btn,
            status_label,
            status_icon: status_icon_label,
            live_cps_label,
            mode_row,
            burst_row,
            button_row,
            jitter_switch,
            jitter_min_row,
            jitter_max_row,
            hotkey_btn,
            hotkey_label,
            toast_overlay,
        }
    }

    pub fn set_cps_display(&self, cps: u32) {
        self.cps_value_label.set_text(&format!("{cps} CPS"));
    }

    /// Update the displayed hotkey name
    pub fn set_hotkey_display(&self, key_name: &str) {
        self.hotkey_label.set_text(key_name);
    }
}
