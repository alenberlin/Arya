import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DictationPanel } from "../dictation/DictationPanel";
import type { DictationSettings } from "../lib/dictation";

const state = {
  // `satisfies` pins this to the real shape, so a future field added to
  // DictationSettings fails the build here instead of testing against undefined.
  settings: {
    shortcut: "ctrl+alt+d",
    mode: "push-to-talk",
    style: "standard",
    polish: "clean",
    language: "en",
    microphone: null,
    speechModel: "whisper-large-v3-turbo-q5_0",
    tone: "neutral",
    streaming: false,
    cleanupModel: null,
    ollamaUrl: "http://127.0.0.1:11434",
    translate: null,
    translateProvider: "local",
    translateModel: null,
  } satisfies DictationSettings,
  saved: [] as unknown[],
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "get_dictation_settings":
        return state.settings;
      case "set_dictation_settings":
        state.saved.push(args?.settings);
        return undefined;
      case "dictation_status":
        return {
          accessibilityTrusted: false,
          recording: false,
          inputDevices: ["MacBook Pro Microphone"],
        };
      case "list_dictation_history":
        return [
          {
            id: "h1",
            rawText: "um hello there",
            cleanText: "Hello there.",
            appBundleId: "com.apple.TextEdit",
            durationMs: 1800,
            asrMs: 120,
            createdAt: "2026-07-03T00:00:00Z",
          },
        ];
      case "list_dictionary_entries":
        return [{ id: "d1", pattern: "k8s", replacement: "Kubernetes" }];
      case "list_speaker_profiles":
        return [{ id: "p1", name: "Alen", createdAt: "2026-07-03T00:00:00Z" }];
      case "list_all_dictation_translations":
        return [];
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  }),
}));

describe("dictation panel", () => {
  beforeEach(() => {
    state.saved = [];
  });

  it("renders settings, history, dictionary, and the accessibility warning", async () => {
    render(<DictationPanel />);
    expect(await screen.findByLabelText("dictation hotkey")).toHaveValue("ctrl+alt+d");
    expect(screen.getByText("Hello there.")).toBeInTheDocument();
    expect(screen.getByText(/k8s → Kubernetes/)).toBeInTheDocument();
    expect(screen.getByText(/Accessibility permission is required/)).toBeInTheDocument();
  });

  it("persists a mode change", async () => {
    const user = userEvent.setup();
    render(<DictationPanel />);
    await screen.findByLabelText("dictation hotkey");
    await user.selectOptions(screen.getByRole("combobox", { name: /^mode$/i }), "toggle");
    await waitFor(() => {
      expect(state.saved).toHaveLength(1);
      expect((state.saved[0] as { mode: string }).mode).toBe("toggle");
    });
  });

  it("persists a recognition model change", async () => {
    const user = userEvent.setup();
    render(<DictationPanel />);
    await screen.findByLabelText("dictation hotkey");

    await user.selectOptions(
      screen.getByRole("combobox", { name: /recognition model/i }),
      "whisper-base.en",
    );

    await waitFor(() => {
      expect(state.saved).toHaveLength(1);
      expect((state.saved[0] as { speechModel: string }).speechModel).toBe("whisper-base.en");
    });
  });
});
