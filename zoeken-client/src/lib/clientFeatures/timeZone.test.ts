import { describe, expect, it } from "vitest";
import { computeTimeZoneAnswer } from "./timeZone";

describe("computeTimeZoneAnswer", () => {
	it("answers for a bare time keyword", () => {
		const answer = computeTimeZoneAnswer("time", 1);
		expect(answer?.engine).toBe("time_zone");
		expect(answer?.answer).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} UTC$/);
	});

	it("answers when every token is a keyword", () => {
		expect(computeTimeZoneAnswer("clock now", 1)?.engine).toBe("time_zone");
	});

	it("is case-insensitive", () => {
		expect(computeTimeZoneAnswer("TIME", 1)?.engine).toBe("time_zone");
	});

	it("returns null when any token isn't a keyword", () => {
		expect(computeTimeZoneAnswer("time Berlin", 1)).toBeNull();
	});

	it("returns null for an empty query", () => {
		expect(computeTimeZoneAnswer("   ", 1)).toBeNull();
	});

	it("returns null past the first page", () => {
		expect(computeTimeZoneAnswer("time", 2)).toBeNull();
	});
});
