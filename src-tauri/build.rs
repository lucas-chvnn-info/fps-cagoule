fn main() {
    // Manifeste administrateur : registre machine, plan d'alimentation et PresentMon en ont besoin
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("échec de tauri-build");
}
