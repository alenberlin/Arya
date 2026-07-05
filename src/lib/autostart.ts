import { invoke } from "@tauri-apps/api/core";

/**
 * Launch-at-login, backed by tauri-plugin-autostart (a macOS LaunchAgent that
 * starts Arya with `--minimized` so it waits in the menu bar).
 */

/** Whether Arya is currently set to start at login. */
export const isAutostartEnabled = () => invoke<boolean>("plugin:autostart|is_enabled");

/** Turn on launch-at-login. */
export const enableAutostart = () => invoke<void>("plugin:autostart|enable");

/** Turn off launch-at-login. */
export const disableAutostart = () => invoke<void>("plugin:autostart|disable");
