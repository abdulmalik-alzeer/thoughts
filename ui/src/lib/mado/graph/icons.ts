// The icons of the diagram, from the app's icon set (Lucide), one per look and per action.
import {
  AppWindow, Split, Zap, Play, Flag, MonitorSmartphone, Server, Database, Cloud, Layers, Table2, KeyRound, Link2, MessageSquare,
  Pencil, Trash2, Undo2, Redo2, RotateCcw, Plus, LayoutDashboard, Maximize, ZoomIn, ZoomOut, List, Waypoints, Box, SquareCheckBig,
  Rows3, X, Check, Tag, Info,
} from '@lucide/svelte';
import type { Component } from 'svelte';

type IconType = Component<{ size?: number | string; strokeWidth?: number | string; class?: string; 'aria-hidden'?: 'true' }>;
const as = (c: unknown) => c as IconType;

export const LOOK_ICON: Record<string, IconType> = {
  screen: as(AppWindow), decision: as(Split), action: as(Zap), start: as(Play), end: as(Flag),
  client: as(MonitorSmartphone), service: as(Server), store: as(Database), external: as(Cloud), queue: as(Layers),
  entity: as(Table2), task: as(SquareCheckBig), other: as(Box),
};
export const lookIcon = (look: string): IconType => LOOK_ICON[look] ?? LOOK_ICON.other;

export const ICON = {
  pk: as(KeyRound), fk: as(Link2), comment: as(MessageSquare), rename: as(Pencil), remove: as(Trash2), undo: as(Undo2), redo: as(Redo2),
  restore: as(RotateCcw), add: as(Plus), tidy: as(LayoutDashboard), fit: as(Maximize), zoomIn: as(ZoomIn), zoomOut: as(ZoomOut),
  list: as(List), map: as(Waypoints), fields: as(Rows3), close: as(X), check: as(Check), label: as(Tag), info: as(Info),
};
