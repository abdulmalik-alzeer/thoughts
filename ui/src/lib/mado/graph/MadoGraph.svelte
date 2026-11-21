<script lang="ts">
  // The interactive diagram of a project plan: one component, four looks (by `doc.kind`). The props are a fixed
  // interface: the Plan view mounts this file. Everything behind it lives in this folder.
  import { SvelteFlowProvider } from '@xyflow/svelte';
  import type { Comment, GraphDoc, GraphOp, Pos } from '../types';
  import GraphCanvas from './GraphCanvas.svelte';

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

<SvelteFlowProvider>
  <GraphCanvas {doc} {comments} {editable} {pendingOps} {onops} {onpositions} {oncomment} {onselect} />
</SvelteFlowProvider>
