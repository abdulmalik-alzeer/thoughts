// Development only: one diagram per kind, each with English or Arabic labels, a very large one and a broken one.
import type { GraphDoc } from '../../types';

type L = [en: string, ar: string];
const pick = (ar: boolean) => (l: L) => l[ar ? 1 : 0];

const flow = (ar: boolean): GraphDoc => {
  const p = pick(ar);
  return {
    version: 1, kind: 'flow', title: p(['Signing in and ordering', 'تسجيل الدخول والطلب']),
    nodes: [
      { id: 'start', label: p(['Opens the app', 'يفتح التطبيق']), kind: 'start' },
      { id: 'welcome', label: p(['Welcome', 'الترحيب']), kind: 'screen' },
      { id: 'has-account', label: p(['Has an account?', 'هل لديه حساب؟']), kind: 'decision' },
      { id: 'sign-in', label: p(['Sign in', 'تسجيل الدخول']), kind: 'screen' },
      { id: 'sign-up', label: p(['Create an account', 'إنشاء حساب جديد']), kind: 'screen' },
      { id: 'send-code', label: p(['Send a code by SMS', 'إرسال رمز التحقق برسالة نصية']), kind: 'action' },
      { id: 'home', label: p(['Home', 'الرئيسية']), kind: 'screen' },
      { id: 'cart', label: p(['Cart, with a rather long name to see how two lines are cut', 'سلة المشتريات باسم طويل نسبياً لنرى كيف يُقطع السطران']), kind: 'screen' },
      { id: 'pay', label: p(['Take the payment', 'تحصيل المبلغ']), kind: 'action' },
      { id: 'paid', label: p(['Payment went through?', 'هل نجح الدفع؟']), kind: 'decision' },
      { id: 'done', label: p(['Order placed', 'تم الطلب']), kind: 'end' },
      { id: 'odd', label: p(['Something of a kind the app does not know', 'عنصر من نوع لا يعرفه التطبيق']), kind: 'portal' },
    ],
    edges: [
      { id: 'e1', from: 'start', to: 'welcome' },
      { id: 'e2', from: 'welcome', to: 'has-account' },
      { id: 'e3', from: 'has-account', to: 'sign-in', label: p(['Yes', 'نعم']) },
      { id: 'e4', from: 'has-account', to: 'sign-up', label: p(['No', 'لا']) },
      { id: 'e5', from: 'sign-up', to: 'send-code' },
      { id: 'e6', from: 'send-code', to: 'home', label: p(['Code is right', 'الرمز صحيح']) },
      { id: 'e7', from: 'sign-in', to: 'home' },
      { id: 'e8', from: 'home', to: 'cart', label: p(['Adds a product', 'يضيف منتجاً']) },
      { id: 'e9', from: 'cart', to: 'pay' },
      { id: 'e10', from: 'pay', to: 'paid' },
      { id: 'e11', from: 'paid', to: 'done', label: p(['Yes', 'نعم']) },
      { id: 'e12', from: 'paid', to: 'cart', label: p(['No, try again', 'لا، حاول مجدداً']) },
      { id: 'e13', from: 'home', to: 'odd' },
    ],
  };
};

const architecture = (ar: boolean): GraphDoc => {
  const p = pick(ar);
  return {
    version: 1, kind: 'architecture', title: p(['How the parts fit together', 'كيف تترابط الأجزاء']),
    nodes: [
      { id: 'ios', label: p(['iPhone app', 'تطبيق الآيفون']), kind: 'client' },
      { id: 'web', label: p(['Web shop', 'المتجر على الويب']), kind: 'client' },
      { id: 'api', label: p(['Shop API', 'واجهة المتجر البرمجية']), kind: 'service' },
      { id: 'orders', label: p(['Orders service', 'خدمة الطلبات']), kind: 'service' },
      { id: 'jobs', label: p(['Order jobs', 'مهام الطلبات']), kind: 'queue' },
      { id: 'db', label: p(['Main database', 'قاعدة البيانات الرئيسية']), kind: 'store' },
      { id: 'files', label: p(['Product photos', 'صور المنتجات']), kind: 'store' },
      { id: 'pay', label: p(['Payment provider', 'مزوّد الدفع']), kind: 'external' },
      { id: 'sms', label: p(['SMS gateway', 'بوابة الرسائل النصية']), kind: 'external' },
    ],
    edges: [
      { id: 'a1', from: 'ios', to: 'api', label: 'HTTPS' }, { id: 'a2', from: 'web', to: 'api', label: 'HTTPS' },
      { id: 'a3', from: 'api', to: 'orders' }, { id: 'a4', from: 'api', to: 'db', label: p(['reads', 'يقرأ']) },
      { id: 'a5', from: 'orders', to: 'jobs', label: p(['puts work on', 'يضيف عملاً إلى']) }, { id: 'a6', from: 'orders', to: 'db', label: p(['writes', 'يكتب']) },
      { id: 'a7', from: 'orders', to: 'pay' }, { id: 'a8', from: 'jobs', to: 'sms' }, { id: 'a9', from: 'api', to: 'files' },
    ],
  };
};

const erd = (ar: boolean): GraphDoc => {
  const p = pick(ar);
  return {
    version: 1, kind: 'erd', title: p(['What is stored', 'ما الذي يُخزَّن']),
    nodes: [
      { id: 'customer', label: p(['Customer', 'العميل']), kind: 'entity', props: { fields: [
        { name: 'id', type: 'uuid', key: 'pk' }, { name: p(['name', 'الاسم']), type: 'text' }, { name: 'phone', type: 'text', note: p(['With the country code', 'مع رمز الدولة']) }, { name: 'created_at', type: 'timestamp' },
      ] } },
      { id: 'order', label: p(['Order', 'الطلب']), kind: 'entity', props: { fields: [
        { name: 'id', type: 'uuid', key: 'pk' }, { name: 'customer_id', type: 'uuid', key: 'fk' }, { name: 'status', type: 'text' }, { name: 'total', type: 'numeric(10,2)' }, { name: 'placed_at', type: 'timestamp' },
      ] } },
      { id: 'line', label: p(['Order line', 'بند الطلب']), kind: 'entity', props: { fields: [
        { name: 'order_id', type: 'uuid', key: 'fk' }, { name: 'product_id', type: 'uuid', key: 'fk' }, { name: 'quantity', type: 'int' },
      ] } },
      { id: 'product', label: p(['Product', 'المنتج']), kind: 'entity', props: { fields: [
        { name: 'id', type: 'uuid', key: 'pk' }, { name: p(['title', 'العنوان']), type: 'text' }, { name: 'price', type: 'numeric(10,2)' }, { name: 'a_field_with_a_very_long_name_indeed', type: 'a_very_long_type_name[]' },
        ...Array.from({ length: 12 }, (_, i) => ({ name: `extra_${i + 1}`, type: 'text' })),
      ] } },
      { id: 'profile', label: p(['Profile', 'الملف الشخصي']), kind: 'entity', props: { fields: [{ name: 'customer_id', type: 'uuid', key: 'pk' }, { name: 'bio', type: 'text' }] } },
      { id: 'tag', label: p(['Tag', 'الوسم']), kind: 'entity' },
    ],
    edges: [
      { id: 'r1', from: 'customer', to: 'order', kind: '1-n', label: p(['places', 'يُنشئ']) },
      { id: 'r2', from: 'order', to: 'line', kind: '1-n' },
      { id: 'r3', from: 'product', to: 'line', kind: '1-n' },
      { id: 'r4', from: 'customer', to: 'profile', kind: '1-1' },
      { id: 'r5', from: 'product', to: 'tag', kind: 'n-n', label: p(['tagged', 'موسوم بـ']) },
    ],
  };
};

const tasks = (ar: boolean): GraphDoc => {
  const p = pick(ar);
  const t = (id: string, label: L, feature: string, wave: number, lane: string) => ({ id, label: p(label), kind: 'task', props: { feature, wave, lane } });
  return {
    version: 1, kind: 'tasks', title: p(['Build plan', 'خطة البناء']),
    nodes: [
      t('project-setup', ['Set the project up', 'تهيئة المشروع'], 'F1', 1, 'a'),
      t('data-model', ['Create the tables', 'إنشاء الجداول'], 'F1', 2, 'a'),
      t('design-tokens', ['Colours and type', 'الألوان والخطوط'], 'F2', 2, 'b'),
      t('sign-in', ['Sign in and sign up', 'تسجيل الدخول وإنشاء الحساب'], 'F2', 3, 'a'),
      t('catalogue', ['Product list and search', 'قائمة المنتجات والبحث'], 'F3', 3, 'b'),
      t('product-page', ['Product page', 'صفحة المنتج'], 'F3', 3, 'b'),
      t('cart', ['Cart', 'سلة المشتريات'], 'F4', 4, 'a'),
      t('checkout', ['Checkout and payment', 'إتمام الطلب والدفع'], 'F4', 5, 'a'),
      t('order-sms', ['Text message when an order is placed', 'رسالة نصية عند إنشاء الطلب'], 'F5', 5, 'b'),
    ],
    edges: [
      { id: 'd1', from: 'project-setup', to: 'data-model' }, { id: 'd2', from: 'project-setup', to: 'design-tokens' },
      { id: 'd3', from: 'data-model', to: 'sign-in' }, { id: 'd4', from: 'data-model', to: 'catalogue' }, { id: 'd5', from: 'design-tokens', to: 'catalogue' },
      { id: 'd6', from: 'data-model', to: 'product-page' }, { id: 'd7', from: 'sign-in', to: 'cart' }, { id: 'd8', from: 'product-page', to: 'cart' },
      { id: 'd9', from: 'cart', to: 'checkout' }, { id: 'd10', from: 'cart', to: 'order-sms' },
    ],
  };
};

/** The largest diagram the contract allows. */
const big = (): GraphDoc => ({
  version: 1, kind: 'flow', title: '400 nodes, 700 edges',
  nodes: Array.from({ length: 400 }, (_, i) => ({ id: `n${i}`, label: `Screen ${i + 1}`, kind: i % 11 === 0 ? 'decision' : i % 7 === 0 ? 'action' : 'screen' })),
  edges: [
    ...Array.from({ length: 399 }, (_, i) => ({ id: `t${i}`, from: `n${Math.floor(i / 3)}`, to: `n${i + 1}`, label: i % 6 ? undefined : 'next' })),
    ...Array.from({ length: 301 }, (_, i) => ({ id: `x${i}`, from: `n${(i * 13 + 5) % 400}`, to: `n${(i * 29 + 17) % 400}` })),
  ],
});

/** What a careless model might write: markup in labels, missing parts, edges to nowhere. */
const broken = (): GraphDoc => ({
  version: 1, kind: 'flow', title: '<script>alert(1)</script> Broken <b>file</b>',
  nodes: [
    { id: 'a', label: '<img src=x onerror="alert(1)"> <b>bold?</b>', kind: 'screen' },
    { id: 'b', label: 'x'.repeat(400), kind: 'screen' },
    { id: 'c', label: 'ok\nwith a line break and a \u202e bidi override', kind: 'screen', pos: { x: 'left', y: null } },
    { id: 'c', label: 'same id again', kind: 'screen' },
    { id: 'Not An Id', label: 'bad id', kind: 'screen' },
    { id: 'd', kind: 42 },
    null, 'text', 7,
  ],
  edges: [
    { id: 'e1', from: 'a', to: 'b', label: '<i>markup</i> & "quotes"' }, { id: 'e2', from: 'a', to: 'nowhere' }, { id: 'e3', from: 'b' }, { from: 'a', to: 'c' },
    { id: 'e4', from: 'b', to: 'c', label: 'y'.repeat(300) }, { id: 'e5', from: 'c', to: 'd' },
  ],
} as unknown as GraphDoc);

export const FIXTURES = { flow, architecture, erd, tasks, big, broken };
export type FixtureName = keyof typeof FIXTURES;
