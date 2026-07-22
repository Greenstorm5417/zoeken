/** Post-search result pipeline: former server plugins, now client-side. */
import type { Config, SearchResult } from "../api";
import { applyDoiRewrite } from "./doiRewrite";
import { applyHostnames, sortByPriority } from "./hostnames";
import { applyTrackerUrlRemover } from "./trackerUrlRemover";

export function pluginEnabled(config: Config | undefined, id: string): boolean {
	return Boolean(config?.plugins?.find((p) => p.id === id)?.enabled);
}

/** Filter/map/re-sort a page of results per the user's enabled client features. */
export function applyClientFeatures(
	results: SearchResult[],
	config: Config | undefined,
): SearchResult[] {
	let working = results;
	if (pluginEnabled(config, "tracker_url_remover")) {
		working = applyTrackerUrlRemover(working);
	}

	const hostnamesOn = pluginEnabled(config, "hostnames");
	const doiOn = pluginEnabled(config, "oa_doi_rewrite");

	const prioritized = hostnamesOn
		? applyHostnames(working, config?.hostnames)
		: working.map((result) => ({ result, priority: "normal" as const }));
	let sorted = sortByPriority(prioritized);

	const resolverUrl = config?.default_doi_resolver
		? config?.doi_resolver_urls?.[config.default_doi_resolver]
		: undefined;
	if (doiOn && resolverUrl) {
		sorted = sorted.map((result) => applyDoiRewrite(result, resolverUrl));
	}

	return sorted;
}
