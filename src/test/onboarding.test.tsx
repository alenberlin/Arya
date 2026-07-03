import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Onboarding, onboardingComplete } from "../onboarding/Onboarding";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "dictation_status") {
      return { accessibilityTrusted: true, recording: false, inputDevices: ["Mic"] };
    }
    return null;
  }),
}));

describe("onboarding", () => {
  afterEach(() => {
    localStorage.clear();
  });

  it("walks welcome → done and marks complete on finish", async () => {
    const user = userEvent.setup();
    const onFinish = vi.fn();
    render(<Onboarding onFinish={onFinish} />);

    expect(screen.getByRole("heading", { name: /Welcome to Arya/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Get started" }));
    expect(screen.getByRole("heading", { name: /Private by design/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    // Permissions step reflects granted state from the mocked status.
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: /Two quick permissions/ })).toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("heading", { name: /Try dictation/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Skip for now/ }));
    expect(screen.getByRole("heading", { name: /You're set/ })).toBeInTheDocument();

    expect(onboardingComplete()).toBe(false);
    await user.click(screen.getByRole("button", { name: /Open Arya/ }));
    expect(onFinish).toHaveBeenCalled();
    expect(onboardingComplete()).toBe(true);
  });

  it("skip onboarding finishes immediately", async () => {
    const user = userEvent.setup();
    const onFinish = vi.fn();
    render(<Onboarding onFinish={onFinish} />);
    await user.click(screen.getByRole("button", { name: "Skip onboarding" }));
    expect(onFinish).toHaveBeenCalled();
  });
});
