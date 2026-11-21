<script lang="ts">
  // Placeholder with the fixed interface. The design worker replaces the body; nobody else edits this file.
  import type { Comment, CommentTarget } from '../types';

  let {
    projectId,
    comments = [],
    locked = false,
    version = 0,
    oncomment,
  }: {
    /** The project whose design this is; the component talks to /api/mado/:projectId/design/* itself. */
    projectId: string;
    /** Comments whose target is a token or a screen: shown as marks and pins. */
    comments?: Comment[];
    /** true: look only (the step is approved or a run is writing). */
    locked?: boolean;
    /** Changes whenever the daemon said the design changed: fetch again. */
    version?: number;
    /** The user asked to comment on a token or dropped a pin on a screen. */
    oncomment?: (target: Extract<CommentTarget, { kind: 'token' | 'screen' }>) => void;
  } = $props();
</script>

<div data-mado-design>{projectId}</div>
