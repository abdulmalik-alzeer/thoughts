// Chat strings added for 0.0.9 (audit fixes in the chat view). Keys are prefixed `chata.`.
export default {
  en: {
    // Labels that used to show as raw words (the chat's kind in its header and in lists; how a reorganize ended).
    'chat.kind.workflow': 'Automatic task', 'chat.kind.module': 'Feature', 'shell.kind.workflow': 'Automatic task',
    'shell.reorg.phase.interrupted': 'Cut off', 'shell.reorg.phase.failed': 'Did not finish',
    'chata.bypassOn': 'Permission checks are off in this chat: the assistant can run commands and change files anywhere on this computer without asking.',
    'chata.bypassOff': 'Turn checks back on',
    'chata.run.wrapPreview': 'Preparing a preview of what will be saved…',
    'chata.run.wrapApply': 'Saving this wrap into your library…',
    'chata.run.reorganize': 'Reorganizing a copy of your library…',
    'chata.run.setup': 'Setting up your library…',
    'chata.stopRun': 'Stop the assistant',
    'chata.tooLargeTogether': '{name} was not added: one message can carry 50 MB of files at most. Send it in a separate message.',
    'chata.deleteBody': 'The conversation and its attached files are deleted from this computer, together with the assistant’s own transcript of it. Anything already saved to your library stays.',
  },
  ar: {
    'chat.kind.workflow': 'مهمة تلقائية', 'chat.kind.module': 'ميزة', 'shell.kind.workflow': 'مهمة تلقائية',
    'shell.reorg.phase.interrupted': 'انقطعت', 'shell.reorg.phase.failed': 'لم تكتمل',
    'chata.bypassOn': 'فحوص الأذونات متوقفة في هذه المحادثة: يستطيع المساعد تشغيل الأوامر وتعديل الملفات في أي مكان على هذا الحاسوب دون أن يستأذن.',
    'chata.bypassOff': 'إعادة تفعيل الفحوص',
    'chata.run.wrapPreview': 'جارٍ تحضير معاينة لما سيُحفظ…',
    'chata.run.wrapApply': 'جارٍ حفظ هذا التلخيص في مكتبتك…',
    'chata.run.reorganize': 'جارٍ إعادة تنظيم نسخة من مكتبتك…',
    'chata.run.setup': 'جارٍ تجهيز مكتبتك…',
    'chata.stopRun': 'إيقاف المساعد',
    'chata.tooLargeTogether': 'لم يُضَف «{name}»: الرسالة الواحدة تتّسع لملفات مجموعها 50 م.ب. على الأكثر. أرسله في رسالة أخرى.',
    'chata.deleteBody': 'ستُحذف المحادثة ومرفقاتها من هذا الجهاز، ومعها نسخة المساعد الخاصة من نصّ المحادثة. ما حفظتَه في مكتبتك يبقى كما هو.',
  },
};
