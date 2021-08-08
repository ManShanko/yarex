use steam::get_steam_apps;

fn main() {
    let mut apps = get_steam_apps().unwrap();
    apps.sort_by(|a, b| a.install_dir.cmp(&b.install_dir));
    println!("{: >10}   {}", "App ID", "Path");
    for app in apps {
        println!("{: >10}   {}", app.app_id, app.install_dir.display());
    }
}
