// 跨页面复用的通用文案。只有一个字符串真的在 3 个以上文件重复出现时才收进这里,
// 避免为了"复用"而过早耦合不同页面各自的措辞。
export const common = {
  durationDaysHours: "{days}天 {hours}小时",
  durationHoursMinutes: "{hours}小时 {minutes}分",
  durationMinutes: "{minutes}分"
};

export type CommonMessages = typeof common;
