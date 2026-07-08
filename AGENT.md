# SnapVault - Project Guidelines

## 🎯 Project Overview
SnapVault is a self-hosted, offline-first desktop application that allows users to view and interact with their exported Snapchat data (memories, chats, profile data). 

This project is a complete rewrite of an existing Dockerized web application (formerly SnapCapsule). The goal of this rewrite is to make the application lightweight, easy to install, and heavily optimized for the average desktop user by eliminating all server-side infrastructure (Docker, PostgreSQL, Redis, Celery).

## 📁 Reference Architecture (The "Old" Web Repo)
The original codebase is located locally at: 
**`C:\Users\Sammy\Documents\GitHub\SnapCapsule-Web-V2`**

Whenever instructed to port a feature, reference this directory. Do not ask for the path again.
* **Frontend Reference:** Look in `apps/web/src/` for the existing React components, custom hooks, and Tailwind styling.
* **Data Models & Logic:** Look in `packages/snapcapsule_core/` for the Python data models and logic.
* **Edge Cases & Fixes:** Look in `scripts/` and `packages/snapcapsule_core/services/` for scripts related to repairing timestamps, backfilling thumbnails, and fixing overlays.

## ⚠️ CRITICAL: Ingestion, File Handling, and Metadata
The most complex and vital part of SnapVault is accurately extracting and processing the Snapchat `.zip` export. Snapchat exports are messy, and the old repository contains specific logic to fix these quirks. When translating the ingestion pipeline to Rust, you MUST strictly adhere to the following:
1.  **JSON over EXIF:** Snapchat media files often lack correct EXIF data. You must rely on the provided `memories_history.json` and other JSON files to derive the absolute truth for timestamps, location data, and tags.
2.  **Timestamp Repair:** The Rust backend must explicitly apply the correct timestamps to the extracted files and the database, recreating the logic previously handled by Python scripts (like `repair_memory_timestamps_from_archives.py`).
3.  **Overlays:** Snapchat saves captions/drawings as separate transparent images. The Rust media processor must composite these overlays onto the base image/video accurately to recreate the original snap.
4.  **Robust ZIP Extraction:** The backend must handle large archives efficiently, keeping memory usage low and streaming progress/status events via Tauri IPC so the frontend can display an accurate progress bar.

## 🛠️ Tech Stack
* **App Framework:** Tauri (v2 recommended)
* **Frontend:** React, TypeScript, Tailwind CSS, Vite
* **Backend:** Rust
* **Database:** SQLite (local file-based via `rusqlite` or `sqlx`)
* **Media Processing:** Bundled `ffmpeg` (or OS-native media APIs) for thumbnail generation, overlay compositing, and video transcoding.

## 🛑 Strict Architectural Rules
1.  **NO Server Infrastructure:** Do not suggest or implement Docker, PostgreSQL, Redis, Celery, or FastAPI. 
2.  **Local File System First:** All data parsing, zip extraction, database creation, and media generation must happen locally in the user's OS-specific AppData / Application Support directory.
3.  **Rust for Heavy Lifting:** All CPU-intensive tasks MUST be written in Rust on background threads. Do not block the React UI thread.
4.  **Tauri IPC:** Communication between the React frontend and the SQLite database/file system must happen strictly via Tauri Commands.
5.  **Cross-Platform Paths:** Because this is currently being developed on Windows, ensure all Rust file system operations use `std::path::PathBuf` and the `tauri::api::path` modules to construct paths safely. Never hardcode `\` or `/` separators.

## 🛡️ Security & Version Control
* **.gitignore:** You must ensure that all sensitive files, generated artifacts, and user data are excluded from version control. This includes local SQLite databases (`.db`, `.sqlite`), environment variables (`.env`), build directories (`target/`, `node_modules/`, `dist/`), and any extracted Snapchat archive data or thumbnails. If a file or folder *can* and *should* be excluded from the GitHub repo, add it to `.gitignore`.

## 🗄️ Database Migration Strategy
The old project used PostgreSQL managed by SQLAlchemy and Alembic. 
* To understand the old schema, look at `packages/snapcapsule_core/models/` in the reference repository.
* Translate these models directly into SQLite table creations in Rust. 
* Remove any Postgres-specific types (like `JSONB`, replacing them with standard `TEXT` strings parsed on the fly).

## 🧠 Coding Guidelines for Claude
* **TypeScript/React:** Use functional components, hooks, and strict typing. 
* **Rust:** Write memory-safe, idiomatic Rust. Handle errors gracefully using `Result` and `Option`. Never use `unwrap()` in production-critical paths; bubble up errors to the frontend using `Result<T, String>`.
* **Logging:** Implement a simple logging layer in Rust and mirror `console.log` in React so we can trace data passing through the IPC bridge and catch ingestion errors early.
* **Incremental Steps:** When asked to implement a feature, break it down. Write the Rust backend command first, ensure it compiles, and *then* write the React frontend binding.

## 🗺️ Implementation Roadmap
1.  **Phase 1: Scaffold & Setup:** Initialize Tauri + React + Vite. Port over the base UI layout components. Configure `.gitignore`.
2.  **Phase 2: Database & State:** Implement SQLite in Rust and create the initial schema. 
3.  **Phase 3: The Ingestion Engine (CRITICAL):** Build the Rust logic to extract a Snapchat `.zip` export. This includes parsing the JSON files, fixing missing timestamps, storing files locally, and populating the SQLite database. Ensure IPC events stream progress to the React UI.
4.  **Phase 4: Media Processing:** Implement the background media processor in Rust (using FFmpeg) to composite overlays, generate thumbnails, and format media for the local webview.
5.  **Phase 5: UI Integration:** Port over the Virtualized Timeline Grid, Lightbox, and Chat views from the reference repo, wiring them up to the new Tauri commands.
