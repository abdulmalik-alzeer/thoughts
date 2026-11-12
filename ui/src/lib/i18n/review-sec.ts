// Final security review (review-sec): the trust question's sentences for Codex's own files, and
// the line in Settings › Assistant that says customising always uses Claude Code.
export default {
  en: {
    'seca.trust.kind.agents_file': 'Standing instructions Codex follows by itself in every chat in this folder',
    'seca.trust.kind.codex_file': 'A Codex settings file for this folder. It can start programs, connect outside tools or send your requests somewhere else.',
    'revsec.codex.customize': 'Customising the app always uses Claude Code, also when Codex answers everything else: Codex could read the app’s own data while it works.',
  },
  ar: {
    'seca.trust.kind.agents_file': 'تعليمات دائمة يتّبعها Codex من تلقاء نفسه في كل محادثة في هذا المجلد',
    'seca.trust.kind.codex_file': 'ملف إعدادات لـ Codex خاص بهذا المجلد. يمكنه تشغيل برامج، أو ربط أدوات خارجية، أو إرسال طلباتك إلى مكان آخر.',
    'revsec.codex.customize': 'يستخدم تخصيص التطبيق Claude Code دائماً، حتى حين يجيب Codex عن كل ما عداه، لأن Codex قد يقرأ بيانات التطبيق نفسه أثناء عمله.',
  },
};
