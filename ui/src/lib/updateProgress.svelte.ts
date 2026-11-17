// How far the download of an app update is, for the bar in Settings › Maintenance. One place
// that stays put: not a notification sent again for every piece that arrives.
export const download = $state({ active: false, got: 0, total: 0 });
