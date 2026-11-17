// Strings for the trust question about git settings read from inside the folder (sec-daemon-askpass).
// The key sits under `seca.trust.kind.` because the trust dialog looks a finding's sentence up by its kind there.
export default {
  en: {
    'seca.trust.kind.git_include_inside': 'Sync settings that are read from a file inside this folder. Anything that syncs in could change them and make programs run, so this folder cannot be trusted while they are there.',
  },
  ar: {
    'seca.trust.kind.git_include_inside': 'إعدادات للمزامنة تُقرأ من ملف داخل هذا المجلد. وأي شيء يصل بالمزامنة قد يغيّرها فيجعل برامج تعمل، لذلك لا يمكن الوثوق بهذا المجلد ما دامت موجودة.',
  },
};
