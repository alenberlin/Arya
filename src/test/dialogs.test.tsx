import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { ConfirmDialog } from "../ui/dialogs";

function ConfirmHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open dialog
      </button>
      <ConfirmDialog
        open={open}
        title="Delete note?"
        message="This cannot be undone."
        confirmLabel="Delete"
        danger
        onConfirm={() => setOpen(false)}
        onCancel={() => setOpen(false)}
      />
    </>
  );
}

describe("dialogs", () => {
  it("traps Tab inside the modal and restores focus to the trigger", async () => {
    const user = userEvent.setup();
    render(<ConfirmHarness />);

    const trigger = screen.getByRole("button", { name: "Open dialog" });
    await user.click(trigger);

    const dialog = await screen.findByRole("dialog", { name: "Delete note?" });
    expect(dialog).toBeInTheDocument();

    const cancel = screen.getByRole("button", { name: "Cancel" });
    const confirm = screen.getByRole("button", { name: "Delete" });
    confirm.focus();

    await user.tab();
    expect(cancel).toHaveFocus();

    await user.tab({ shift: true });
    expect(confirm).toHaveFocus();

    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Delete note?" })).not.toBeInTheDocument();
    });
    expect(trigger).toHaveFocus();
  });
});
