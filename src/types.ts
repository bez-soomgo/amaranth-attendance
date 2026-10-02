export type ItemRule =
  | "general"
  | "bonus4h"
  | "leave8h"
  | "deduct_requested"
  | "exclude";

export type DayKind = "work" | "leave" | "inProgress" | "incomplete" | "excluded";

export interface DayRow {
  date: string;
  weekday: string;
  item: string;
  kind: DayKind;
  actualIn: string | null;
  recognizedIn: string | null;
  out: string | null;
  workedMin: number | null;
  breakMin: number;
  bonusMin: number;
  finalMin: number | null;
  note: string;
}

export interface Summary {
  workDays: number;
  actualDays: number;
  leaveDays: number;
  targetMin: number;
  totalMin: number;
  diffMin: number;
}

export interface Report {
  period: string;
  rows: DayRow[];
  summary: Summary;
  notes: string[];
  inProgressArrival: string | null;
}

export type Analysis =
  | { status: "needsRules"; unknownItems: string[] }
  | { status: "ready"; report: Report };

export interface TodayPlan {
  neededMin: number;
  workNeededMin: number;
  recognizedIn: string;
  leaveAt: string | null;
  breakApplied: boolean;
  alreadyMet: boolean;
  halfDayNotice: boolean;
}
