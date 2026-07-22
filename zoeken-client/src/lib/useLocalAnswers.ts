/** Calculator/time/self-info/statistics/random/date-math/crypto answers, computed locally. */
import { useMemo } from "react";
import type { Config, SearchAnswer } from "./api";
import { pluginEnabled } from "./clientFeatures";
import { computeCalculatorAnswer } from "./clientFeatures/calculator";
import { computeCryptoAnswer } from "./clientFeatures/crypto";
import { computeDateTimeAnswer } from "./clientFeatures/dateTime";
import { computeRandomAnswer } from "./clientFeatures/random";
import { computeSelfInfoAnswer } from "./clientFeatures/selfInfo";
import { computeStatisticsAnswer } from "./clientFeatures/statistics";
import { computeTimeZoneAnswer } from "./clientFeatures/timeZone";
import { computeUnitConverterAnswer } from "./clientFeatures/unitConverter";

export function useLocalAnswers(
	q: string,
	language: string | undefined,
	pageno: number,
	config: Config | undefined,
): SearchAnswer[] {
	// biome-ignore lint/correctness/useExhaustiveDependencies: navigator.userAgent is stable per session
	return useMemo(() => {
		const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
		return [
			pluginEnabled(config, "calculator")
				? computeCalculatorAnswer(q, language ?? "", pageno)
				: null,
			pluginEnabled(config, "time_zone") ? computeTimeZoneAnswer(q, pageno) : null,
			pluginEnabled(config, "self_info")
				? computeSelfInfoAnswer(q, pageno, config?.client_ip ?? null, ua)
				: null,
			pluginEnabled(config, "unit_converter")
				? computeUnitConverterAnswer(q, pageno)
				: null,
			computeStatisticsAnswer(q),
			computeRandomAnswer(q),
			computeDateTimeAnswer(q),
			computeCryptoAnswer(q),
		].filter((answer) => answer !== null);
	}, [q, language, pageno, config]);
}
