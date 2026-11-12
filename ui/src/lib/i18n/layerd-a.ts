// Customisation layer: what the review says about a change (where it came from, whether it still
// fits, whether the assistant finished, what its styles do, what a pack says about itself), and
// the app-history list.
export default {
  en: {
    'lda.origin.manual': 'Changed outside the app',

    'lda.staleTitle': 'This change no longer fits your app',
    'lda.staleBody': 'After it was prepared, something in your app was undone, restored or updated, and the two cannot be combined. Discard this change and ask for it again.',
    'lda.staleKeep': 'This change cannot be kept because your app has changed since it was prepared.',

    'lda.incompleteTitle': 'The assistant did not finish this change',
    'lda.incompleteBody': 'It was stopped, or ran into a problem, before it was done. Parts of the change may be missing or may not work. The safest choice is to discard it and ask again.',
    'lda.incompleteAccept': 'I understand it may be incomplete. Let me keep it as it is.',
    'lda.incompleteKeep': 'Tick the box above if you want to keep this change although it may be incomplete.',

    'lda.packSays': 'What this pack says about itself',
    'lda.packSaysNote': 'These are the words of whoever made the pack. Thoughts has not checked them.',
    'lda.packSaysIntro': 'The pack describes itself like this (its maker’s words, not checked by Thoughts):',

    'lda.css.title': 'How it changes the look',
    'lda.css.colours': 'Colours and style settings only: it changes colours, shadows and corner shapes, nothing else on screen.',
    'lda.css.removes': 'It only takes out style changes you had; it adds none.',
    'lda.css.layout': 'More than colours: it also changes where things sit, how big they are, or whether they show.',
    'lda.css.warnTitle': 'The style changes do more than restyle',
    'lda.css.warnBody': 'A look that only changes colours never needs these. Keep this change only if they are what you asked for:',
    'lda.css.flag.text': 'It prints words or symbols of its own on the app’s screens. They would look as if the app had written them.',
    'lda.css.flag.overlay': 'It places things on top of other parts of the app, where they can cover what is underneath.',
    'lda.css.flag.dialogs': 'It restyles the app’s own questions and warnings, such as the boxes that ask you to confirm or to trust something.',
    'lda.css.flag.hides': 'It makes parts of the app invisible.',
    'lda.css.flag.remote': 'It tries to load a picture, a font or another style file. Customisations are not allowed to do that.',
    'lda.css.unreadableTitle': 'The style changes could not be read',
    'lda.css.unreadableBody': 'The style file is damaged or written in an unusual way, so Thoughts cannot tell you what it does. It could change anything on screen. Keep it only if you trust where it came from.',

    'lda.outsidePrivate_one': 'and {n} file in your private folders (its name is not shown)',
    'lda.outsidePrivate_other': 'and {n} files in your private folders (their names are not shown)',

    'lda.hist.older': 'Show older changes',
    'lda.hist.olderLoading': 'Loading older changes…',
    'lda.hist.olderFailed': 'Couldn’t load older changes.',
    'lda.hist.start': 'Where your app started',
    'lda.hist.system': 'Done by the app itself',
    'lda.hist.end': 'That is the whole history, back to where your app started.',
  },
  ar: {
    'lda.origin.manual': 'تغيّر من خارج التطبيق',

    'lda.staleTitle': 'لم يعد هذا التغيير متوافقاً مع تطبيقك',
    'lda.staleBody': 'بعد إعداد هذا التغيير حدث في تطبيقك تراجع أو استعادة أو تحديث، ولا يمكن الجمع بين الاثنين. تجاهل هذا التغيير ثم اطلبه من جديد.',
    'lda.staleKeep': 'لا يمكن اعتماد هذا التغيير لأن تطبيقك تغيّر منذ إعداده.',

    'lda.incompleteTitle': 'لم يُكمل المساعد هذا التغيير',
    'lda.incompleteBody': 'توقّف المساعد، أو واجه مشكلة، قبل أن ينتهي. قد تكون أجزاء من التغيير ناقصة أو لا تعمل. الخيار الأسلم أن تتجاهله وتطلبه من جديد.',
    'lda.incompleteAccept': 'أعلم أنه قد يكون ناقصاً، وأريد اعتماده كما هو.',
    'lda.incompleteKeep': 'ضع علامة في المربع أعلاه إن أردت اعتماد هذا التغيير مع احتمال أنه ناقص.',

    'lda.packSays': 'ما تقوله هذه الحزمة عن نفسها',
    'lda.packSaysNote': 'هذا كلام من أعدّ الحزمة، ولم يتحقق Thoughts منه.',
    'lda.packSaysIntro': 'تصف الحزمة نفسها هكذا (بكلام من أعدّها، ولم يتحقق Thoughts منه):',

    'lda.css.title': 'كيف يغيّر المظهر',
    'lda.css.colours': 'ألوان وإعدادات شكلية فقط: يغيّر الألوان والظلال واستدارة الزوايا، ولا شيء غير ذلك على الشاشة.',
    'lda.css.removes': 'يزيل تغييرات شكلية كانت لديك فقط، ولا يضيف جديداً.',
    'lda.css.layout': 'أكثر من ألوان: يغيّر أيضاً مواضع العناصر أو أحجامها أو ظهورها.',
    'lda.css.warnTitle': 'تغييرات المظهر تتجاوز تغيير الشكل',
    'lda.css.warnBody': 'المظهر الذي يغيّر الألوان فقط لا يحتاج إلى شيء من هذا. لا تعتمد التغيير إلا إذا كان هذا ما طلبته:',
    'lda.css.flag.text': 'يكتب كلمات أو رموزاً من عنده على شاشات التطبيق، فتبدو كأن التطبيق هو من كتبها.',
    'lda.css.flag.overlay': 'يضع عناصر فوق أجزاء أخرى من التطبيق، فقد تغطي ما تحتها.',
    'lda.css.flag.dialogs': 'يغيّر شكل أسئلة التطبيق وتحذيراته، مثل النوافذ التي تطلب منك التأكيد أو الثقة بشيء.',
    'lda.css.flag.hides': 'يُخفي أجزاء من التطبيق.',
    'lda.css.flag.remote': 'يحاول تحميل صورة أو خط أو ملف مظهر آخر، وهذا غير مسموح للتخصيصات.',
    'lda.css.unreadableTitle': 'تعذّرت قراءة تغييرات المظهر',
    'lda.css.unreadableBody': 'ملف المظهر تالف أو مكتوب بطريقة غير معتادة، فلا يستطيع Thoughts أن يخبرك بما يفعله. قد يغيّر أي شيء على الشاشة. لا تعتمده إلا إذا كنت تثق بمصدره.',

    'lda.outsidePrivate_one': 'وملف واحد في مجلداتك الخاصة (لا يُعرض اسمه)',
    'lda.outsidePrivate_two': 'وملفان في مجلداتك الخاصة (لا يُعرض اسماهما)',
    'lda.outsidePrivate_few': 'و{n} ملفات في مجلداتك الخاصة (لا تُعرض أسماؤها)',
    'lda.outsidePrivate_many': 'و{n} ملفاً في مجلداتك الخاصة (لا تُعرض أسماؤها)',
    'lda.outsidePrivate_other': 'و{n} ملف في مجلداتك الخاصة (لا تُعرض أسماؤها)',

    'lda.hist.older': 'عرض تغييرات أقدم',
    'lda.hist.olderLoading': 'جارٍ تحميل تغييرات أقدم…',
    'lda.hist.olderFailed': 'تعذّر تحميل التغييرات الأقدم.',
    'lda.hist.start': 'نقطة بداية تطبيقك',
    'lda.hist.system': 'أجراه التطبيق نفسه',
    'lda.hist.end': 'هذا هو السجل كاملاً، حتى نقطة بداية تطبيقك.',
  },
};
