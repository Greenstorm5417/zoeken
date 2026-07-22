import { describe, expect, it } from "vitest";
import { computeCalculatorAnswer } from "./calculator";

describe("computeCalculatorAnswer", () => {
	it("computes a simple expression", () => {
		const answer = computeCalculatorAnswer("2 + 2", "en", 1);
		expect(answer?.answer).toBe("4");
		expect(answer?.interactive).toEqual({
			type: "calculator",
			expression: "2 + 2",
			result: 4,
		});
	});

	it("normalizes comma-decimal locales", () => {
		const answer = computeCalculatorAnswer("1,5 + 2,5", "de-DE", 1);
		expect(answer?.answer).toBe("4");
	});

	it("strips thousands commas for non-comma-decimal locales", () => {
		const answer = computeCalculatorAnswer("1,000 + 1", "en", 1);
		expect(answer?.answer).toBe("1001");
	});

	it("returns null for plain text queries", () => {
		expect(computeCalculatorAnswer("weather berlin", "en", 1)).toBeNull();
	});

	it("returns null for a bare number with no operator", () => {
		expect(computeCalculatorAnswer("42", "en", 1)).toBeNull();
	});

	it("returns null past the first page", () => {
		expect(computeCalculatorAnswer("2 + 2", "en", 2)).toBeNull();
	});

	it("returns null for invalid expressions", () => {
		expect(computeCalculatorAnswer("2 + * 2", "en", 1)).toBeNull();
	});
});
