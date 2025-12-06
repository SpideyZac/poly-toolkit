use tauri_plugin_updater::UpdaterExt;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[allow(unused_variables)]
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                #[cfg(not(debug_assertions))]
                update(handle).await.unwrap();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[allow(dead_code)]
async fn update(app: tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(update) = app.updater_builder().build()?.check().await? {
        let mut downloaded = 0;

        update
            .download_and_install(
                |chunk_length, content_length| {
                    downloaded += chunk_length;
                    println!("downloaded {downloaded} from {content_length:?}");
                },
                || {
                    println!("download finished");
                },
            )
            .await?;

        println!("update installed");
        app.restart();
    }

    Ok(())
}
