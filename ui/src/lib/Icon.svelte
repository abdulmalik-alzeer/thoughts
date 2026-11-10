<script lang="ts" module>
  import {
    House, MessageSquare, Lightbulb, ListChecks, FileText, FolderKanban, CircleUser, Settings, RefreshCw,
    History, Sparkles, Plus, Paperclip, Trash2, ArrowUp, Square, Wrench, Brain, TriangleAlert, Check,
    ChevronRight, ChevronDown, Sun, Moon, SunMoon, Search, ArrowRight, ArrowDown, MessageCircle, Folder,
    Puzzle, Bot, GitBranch, Server, Palette, LayoutGrid, X, Inbox, CalendarDays, Clock, BookOpen, Shield,
    Download, Pencil, ExternalLink,
  } from '@lucide/svelte';

  // One place for every icon the app uses. Plugins can pass any of these names.
  export const icons: Record<string, any> = {
    home: House, chats: MessageSquare, chat: MessageCircle, ideas: Lightbulb, todos: ListChecks, docs: FileText,
    projects: FolderKanban, me: CircleUser, settings: Settings, sync: RefreshCw, rollback: History, wrap: Sparkles,
    plus: Plus, attach: Paperclip, trash: Trash2, send: ArrowUp, stop: Square, tool: Wrench, thinking: Brain,
    warning: TriangleAlert, check: Check, right: ChevronRight, down: ChevronDown, sun: Sun, moon: Moon,
    system: SunMoon, search: Search, arrow: ArrowRight, arrowDown: ArrowDown, folder: Folder, plugin: Puzzle,
    ai: Bot, git: GitBranch, daemon: Server, palette: Palette, modules: LayoutGrid, close: X, inbox: Inbox,
    calendar: CalendarDays, clock: Clock, book: BookOpen, shield: Shield, download: Download, edit: Pencil,
    external: ExternalLink,
  };
</script>

<script lang="ts">
  let { name, size = 16, stroke = 1.75, class: cls = '' }: { name: string; size?: number; stroke?: number; class?: string } = $props();
  const C = $derived(icons[name] ?? Puzzle);
</script>

<C {size} strokeWidth={stroke} class="icon {cls}" aria-hidden="true" />
