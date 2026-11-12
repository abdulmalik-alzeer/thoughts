// Strings added by the follow-ups before 0.0.9 (agent `cleanup`).
export default {
  en: {
    // Record a meeting: where the recording goes. The sound file is kept out of the history (and so
    // out of every backup the app sends); only the transcript note is recorded.
    'cln.voice.where': 'The recording is saved in the meetings of “{where}” and turned into text on this Mac; nothing is sent anywhere. The sound file itself stays on this computer: it never goes into your history or your backup, only its transcript does.',
    // The same, for a library kept in a cloud drive's folder (iCloud Drive, Dropbox…), which copies every file in it.
    'cln.voice.whereCloud': 'The recording is saved in the meetings of “{where}” and turned into text on this Mac; Thoughts sends nothing anywhere. The sound file never goes into your history, but your library is in a cloud drive’s folder, and that drive copies it like any other file.',
    // An add-on that lists the folders it reads: what it may do to files where it saves.
    'cln.addon.changeOnly': 'changes or deletes files only inside {folders}; elsewhere it can only add new files',
    'cln.addon.changeOnlyLine': 'Changes or deletes files only inside: {folders}. Elsewhere it can only add new files.',
    // Add to my calendar: what the calendar file leaves out.
    'cln.cal.private': 'Todos in folders you marked private are never included.',
    // Move to another computer, with several libraries: the file holds the open one only.
    'cln.move.oneLibrary': 'The file holds only “{name}”, the library open now: its chats and settings. Your other libraries stay on this computer; open one and save a move file of its own to take it along.',
  },
  ar: {
    'cln.voice.where': 'يُحفظ التسجيل ضمن اجتماعات «{where}» ويُحوَّل إلى نص على هذا الجهاز، ولا يُرسَل شيء إلى أي مكان. ويبقى الملف الصوتي نفسه على هذا الجهاز وحده: لا يدخل سجل مكتبتك ولا نسختها الاحتياطية، وإنما يدخلهما تفريغه النصي.',
    'cln.voice.whereCloud': 'يُحفظ التسجيل ضمن اجتماعات «{where}» ويُحوَّل إلى نص على هذا الجهاز، ولا يرسل Thoughts شيئاً إلى أي مكان. لا يدخل الملف الصوتي سجل مكتبتك، لكن مكتبتك في مجلد تزامنه خدمة سحابية، وهذه الخدمة تنسخه كأي ملف آخر.',
    'cln.addon.changeOnly': 'لا تعدّل الملفات ولا تحذفها إلا داخل {folders}؛ وفي غيرها لا تستطيع إلا إضافة ملفات جديدة',
    'cln.addon.changeOnlyLine': 'لا تعدّل الملفات ولا تحذفها إلا داخل: {folders}. وفي غيرها لا تستطيع إلا إضافة ملفات جديدة.',
    'cln.cal.private': 'المهام الموجودة في المجلدات التي جعلتها خاصة لا تدخل ملف التقويم أبداً.',
    'cln.move.oneLibrary': 'لا يحمل الملف إلا «{name}»، المكتبة المفتوحة الآن: محادثاتها وإعداداتها. أما مكتباتك الأخرى فتبقى على هذا الجهاز؛ افتح أيّاً منها واحفظ لها ملف نقل خاصاً بها لتأخذها معك.',
  },
};
