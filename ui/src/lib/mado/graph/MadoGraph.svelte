<script lang="ts">
  // Placeholder with the fixed interface. The diagram worker replaces the body; nobody else edits this file.
  import type { Comment, GraphDoc, GraphOp, Pos } from '../types';

  let {
    doc,
    comments = [],
    editable = true,
    pendingOps = [],
    onops,
    onpositions,
    oncomment,
    onselect,
  }: {
    /** What to draw. A new object means: draw again, keeping the view where it can. */
    doc: GraphDoc;
    /** Comments whose target is a node or an edge of this diagram: shown as marks on them. */
    comments?: Comment[];
    /** false: select and comment only (the tasks graph, an approved step). */
    editable?: boolean;
    /** Edits the user made that were not sent yet: drawn as pending (added, removed, renamed). */
    pendingOps?: GraphOp[];
    /** The user edited the diagram: the full list of pending edits, to keep until they are sent. */
    onops?: (ops: GraphOp[]) => void;
    /** The user moved nodes: save these positions (presentation only). */
    onpositions?: (positions: Record<string, Pos>) => void;
    /** The user asked to comment on a node or an edge. */
    oncomment?: (target: { kind: 'node'; nodeId: string } | { kind: 'edge'; edgeId: string }) => void;
    /** The selection changed. */
    onselect?: (sel: { kind: 'node' | 'edge'; id: string } | null) => void;
  } = $props();
</script>

<div data-mado-graph>{doc.title}</div>
