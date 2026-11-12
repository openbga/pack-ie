import { matchPath, type RouteObject } from "react-router";
import { describe, expect, it } from "vitest";
import { SETTINGS_TAB_IDS } from "@/features/settings/SettingsPage";
import { routes } from "../routes";
import { articleForPath, articles } from "./articles";

const MAX_PARAGRAPH_CHARACTERS = 600;
const MAX_ARTICLE_WORDS = 300;

/** Every route pattern of the app, except the redirects of `/` and unknown paths. */
function patternsOf(list: readonly RouteObject[]): string[] {
  return list.flatMap((route) => [
    ...(route.path !== undefined && route.path !== "/" && route.path !== "*" ? [route.path] : []),
    ...patternsOf(route.children ?? []),
  ]);
}

const patterns = patternsOf(routes);

/** Real locations of the patterns: ids are made up, every settings tab is visited. */
const locations = patterns.flatMap((pattern) =>
  pattern.includes(":tab")
    ? SETTINGS_TAB_IDS.map((tab) => pattern.replace(":tab", tab))
    : [pattern.replace(":id", "example-1")],
);

const paragraphs = articles.flatMap((article) =>
  article.sections.flatMap((section) =>
    section.paragraphs.map((text) => ({ article: article.id, text })),
  ),
);

describe("help articles", () => {
  it("finds the routes of the app", () => {
    expect(patterns).toContain("/dashboard");
    expect(patterns).toContain("/channels/:id/edit");
    expect(locations).toContain("/settings/secrets");
  });

  it.each(locations)("%s has an article", (location) => {
    expect(articleForPath(location), `no help article for ${location}`).toBeDefined();
  });

  it("gives a page its most specific article", () => {
    expect(articleForPath("/channels/new")?.id).toBe("channel-edit");
    expect(articleForPath("/channels/example-1/edit")?.id).toBe("channel-edit");
    expect(articleForPath("/endpoints/new")?.id).toBe("endpoint-edit");
    expect(articleForPath("/endpoints/example-1")?.id).toBe("endpoint");
    expect(articleForPath("/settings/versions")?.id).toBe("config-versions");
    expect(articleForPath("/settings/secrets")?.id).toBe("settings");
  });

  it("names only routes that exist", () => {
    for (const article of articles) {
      for (const route of article.routes) {
        const exists = patterns.some(
          (pattern) => pattern === route || matchPath({ path: pattern, end: true }, route) !== null,
        );
        expect(exists, `${article.id} names ${route}, which is not a route`).toBe(true);
      }
    }
  });

  it("has unique ids and titles", () => {
    const ids = articles.map((article) => article.id);
    expect(new Set(ids).size).toBe(ids.length);
    const titles = articles.map((article) => article.title);
    expect(new Set(titles).size).toBe(titles.length);
  });

  it.each(articles.map((article) => [article.id, article] as const))(
    "%s is complete and short",
    (_id, article) => {
      expect(article.title.trim()).not.toBe("");
      expect(article.summary.trim()).not.toBe("");
      expect(article.sections.length).toBeGreaterThan(0);
      let words = 0;
      for (const section of article.sections) {
        expect(section.title.trim(), `${article.id} has a section without a title`).not.toBe("");
        expect(section.paragraphs.length).toBeGreaterThan(0);
        for (const text of section.paragraphs) {
          expect(text.trim(), `${article.id} has an empty paragraph`).not.toBe("");
          expect(text.length).toBeLessThanOrEqual(MAX_PARAGRAPH_CHARACTERS);
          words += text.split(/\s+/).length;
        }
      }
      expect(words).toBeLessThanOrEqual(MAX_ARTICLE_WORDS);
    },
  );

  it("never says the same thing in two places", () => {
    const seen = new Map<string, string>();
    for (const { article, text } of paragraphs) {
      const key = text.trim().toLowerCase();
      const earlier = seen.get(key);
      expect(earlier, `${article} repeats a paragraph of ${earlier}`).toBeUndefined();
      seen.set(key, article);
    }
  });
});
