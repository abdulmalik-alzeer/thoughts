// Strings for plan usage shown before the first message (round 0.7.2): the service asks Claude Code
// and Codex themselves how much of the plan is used, so the bars are there when the app opens.
export default {
  en: {
    'usage72.loading': 'Checking with {name}…',
    'usage72.failed': 'Could not check with {name} just now. Thoughts tries again by itself.',
    'usage72.old': 'This version of {name} only reports plan usage with its answers, so it is shown after your next chat. Update {name} to see it as soon as Thoughts opens.',
    'usage72.noReport': '{name} did not report any plan limits for this account.',
    'usage72.service': 'You use another service here, so there is no plan limit. What you used through Thoughts is below.',
    'usage72.key': 'You use an API key here, so there is no plan limit: you pay for what you use. What you used through Thoughts is below.',
    'usage72.set.title': 'Keep plan usage up to date',
    'usage72.set.on': 'Check plan usage automatically',
    'usage72.set.hint': 'When Thoughts opens, every few minutes while it is open and after each chat, Thoughts asks Claude Code and Codex how much of your plan is used. Nothing is sent to a model and no chat is created. When this is off, the figures only arrive with the answers to your chats.',
  },
  ar: {
    'usage72.loading': 'جارٍ الاستعلام من {name}…',
    'usage72.failed': 'تعذّر الاستعلام من {name} الآن. سيحاول Thoughts مجدداً من تلقاء نفسه.',
    'usage72.old': 'هذا الإصدار من {name} لا يذكر استخدام الخطة إلا مع إجاباته، فيظهر بعد محادثتك القادمة. حدّث {name} ليظهر فور فتح Thoughts.',
    'usage72.noReport': 'لم يذكر {name} أي حدود للخطة في هذا الحساب.',
    'usage72.service': 'تستخدم هنا خدمة أخرى، فلا حدّ للخطة. ما استخدمته عبر Thoughts معروض في الأسفل.',
    'usage72.key': 'تستخدم هنا مفتاح API، فلا حدّ للخطة: تدفع مقابل ما تستخدمه. ما استخدمته عبر Thoughts معروض في الأسفل.',
    'usage72.set.title': 'إبقاء استخدام الخطة محدَّثاً',
    'usage72.set.on': 'التحقق من استخدام الخطة تلقائياً',
    'usage72.set.hint': 'عند فتح Thoughts، وكل بضع دقائق ما دام مفتوحاً، وبعد كل محادثة، يسأل Thoughts كلاً من Claude Code و Codex عن مقدار ما استُخدم من خطتك. لا يُرسَل شيء إلى أي نموذج ولا تُنشأ أي محادثة. عند إيقاف هذا الخيار تصل الأرقام مع إجابات محادثاتك فقط.',
  },
};
