pub mod foundry;

use std::sync::Arc;

use foundry::AppState;
use foundry::attachments::AttachmentStore;
use foundry::commands::{
    attachment_data, attachment_discard, attachment_store, cache_clear_all, cache_info,
    cache_set_directory, catalog_list, catalog_model, chat_cancel_generation,
    chat_conversation_usage, chat_create_conversation, chat_delete_conversation, chat_edit_message,
    chat_export, chat_list_conversations, chat_list_messages, chat_regenerate,
    chat_rename_conversation, chat_search, chat_send_message, chat_set_conversation_archived,
    chat_set_conversation_favorite, chat_set_model, cli_detect, cli_status, config_set,
    config_show, diagnostics_report, diagnostics_snapshot, embeddings_answer,
    embeddings_cancel_vectorize, embeddings_compare, embeddings_create_corpus,
    embeddings_delete_corpus, embeddings_get_corpus, embeddings_import_lines,
    embeddings_list_corpora, embeddings_rename_corpus, embeddings_replace_documents,
    embeddings_reveal_source, embeddings_search, embeddings_set_corpus_archived,
    embeddings_set_corpus_favorite, embeddings_set_model, embeddings_vectorize, loaded_models,
    local_models, model_download, model_download_cancel, model_jobs, model_load, model_register,
    model_remove, model_unload, model_unregister, params_set_layer, params_snapshot,
    server_get_preferences, server_logs, server_restart, server_set_preferences, server_start,
    server_status, server_stop, speech_audio_data, speech_delete_transcription,
    speech_discard_audio, speech_get_storage_settings, speech_import, speech_list_input_devices,
    speech_list_transcriptions, speech_microphone_permission, speech_open_microphone_settings,
    speech_open_storage_folder, speech_record_cancel, speech_record_level, speech_record_start,
    speech_record_stop, speech_save_text, speech_send_to_chat, speech_set_storage_settings,
    speech_set_transcription_archived, speech_set_transcription_favorite, speech_transcribe,
    speech_update_transcription, workspace_snapshot,
};
use foundry::db::Database;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("foundryui.sqlite3");
            let database = Database::open(&path)?;
            let attachments = AttachmentStore::new(app.path().app_data_dir()?.join("attachments"));

            let _ = attachments.sweep(&database);

            app.state::<AppState>().install_database(Arc::new(database));
            app.state::<AppState>()
                .install_attachments(Arc::new(attachments));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cli_detect,
            server_status,
            server_start,
            server_stop,
            server_restart,
            server_logs,
            cli_status,
            server_get_preferences,
            server_set_preferences,
            config_show,
            config_set,
            diagnostics_report,
            diagnostics_snapshot,
            catalog_list,
            catalog_model,
            workspace_snapshot,
            model_download,
            model_download_cancel,
            model_jobs,
            loaded_models,
            model_load,
            model_unload,
            model_remove,
            cache_info,
            cache_clear_all,
            cache_set_directory,
            local_models,
            model_register,
            model_unregister,
            chat_list_conversations,
            chat_create_conversation,
            chat_rename_conversation,
            chat_delete_conversation,
            chat_set_conversation_archived,
            chat_set_conversation_favorite,
            chat_set_model,
            chat_list_messages,
            chat_conversation_usage,
            chat_send_message,
            chat_regenerate,
            chat_edit_message,
            chat_search,
            chat_export,
            chat_cancel_generation,
            params_snapshot,
            params_set_layer,
            attachment_store,
            attachment_discard,
            attachment_data,
            speech_import,
            speech_list_input_devices,
            speech_record_start,
            speech_record_level,
            speech_record_stop,
            speech_record_cancel,
            speech_discard_audio,
            speech_audio_data,
            speech_microphone_permission,
            speech_open_microphone_settings,
            speech_get_storage_settings,
            speech_set_storage_settings,
            speech_open_storage_folder,
            speech_transcribe,
            speech_list_transcriptions,
            speech_update_transcription,
            speech_set_transcription_favorite,
            speech_set_transcription_archived,
            speech_delete_transcription,
            speech_save_text,
            speech_send_to_chat,
            embeddings_list_corpora,
            embeddings_set_corpus_favorite,
            embeddings_set_corpus_archived,
            embeddings_get_corpus,
            embeddings_import_lines,
            embeddings_reveal_source,
            embeddings_create_corpus,
            embeddings_rename_corpus,
            embeddings_replace_documents,
            embeddings_set_model,
            embeddings_delete_corpus,
            embeddings_vectorize,
            embeddings_cancel_vectorize,
            embeddings_search,
            embeddings_answer,
            embeddings_compare
        ])
        .build(tauri::generate_context!())
        .expect("error while building FoundryUI")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(state) = app.try_state::<AppState>()
            {
                state.shutdown();
            }
        });
}
