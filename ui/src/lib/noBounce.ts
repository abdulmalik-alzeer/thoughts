// Elastic scrolling off inside a frame. A frame is a document of its own, so the app's global rule
// (theme.css, "No elastic scrolling anywhere") does not reach it: every place that writes the styles
// of a frame's document puts this first. It is a style rule and nothing else: every frame policy
// already allows inline styles, so no sandbox or policy changes for it. Rules of the page itself
// come later and win, so a page may still choose otherwise for a strip that only scrolls sideways.
export const NO_BOUNCE = 'html,body,*,*::before,*::after{overscroll-behavior:none}';
