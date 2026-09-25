// Bound live timers in long lists and conversations. Older entries show the
// exact time instead of a relative label that would stop updating.
export function recentTimestamp(positionFromLatest: number) {
  return positionFromLatest < 10
    ? { format: "relative_short" as const, isLive: true }
    : { format: "date_time" as const, isLive: false };
}
