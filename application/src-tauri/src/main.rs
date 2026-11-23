// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(any(not(debug_assertions), feature = "preview"), windows_subsystem = "windows")]

fn main() {
  scrcpyplus_lib::run();
}
