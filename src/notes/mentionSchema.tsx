import { BlockNoteSchema, defaultInlineContentSpecs } from "@blocknote/core";
import { createReactInlineContentSpec } from "@blocknote/react";

/**
 * The `@`-mention inline content (F1): a reference to another node in the
 * connected brain, carrying the target `(kind, id)` and a display `label`. It
 * renders as a chip; navigation is handled by delegation in the editor host
 * (the chip exposes `data-kind`/`data-id`), keeping this spec static.
 */
export const Mention = createReactInlineContentSpec(
  {
    type: "mention",
    propSchema: {
      kind: { default: "note" },
      id: { default: "" },
      label: { default: "" },
    },
    content: "none",
  },
  {
    render: (props) => {
      const { kind, id, label } = props.inlineContent.props;
      return (
        <span className="mention-chip" data-kind={kind} data-id={id} title={`Open ${label}`}>
          @{label}
        </span>
      );
    },
  },
);

/** The notes editor schema: the BlockNote defaults plus the mention inline type. */
export const notesSchema = BlockNoteSchema.create({
  inlineContentSpecs: {
    ...defaultInlineContentSpecs,
    mention: Mention,
  },
});
