fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "status",
            "focus_catalog",
            "focus_history",
            "study_active",
            "study_start",
            "study_stop",
            "revoke_capture",
            "review_queue",
            "session_begin",
            "answer",
            "session_end",
            "challenges",
            "next_hint",
            "solve",
            "export_files",
            "blocked",
            "block_add",
            "block_rm",
        ]),
    ))
    .expect("не удалось подготовить сборку Tauri");
}
