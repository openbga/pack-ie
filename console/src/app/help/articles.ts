import { matchPath } from "react-router";

interface HelpSection {
  title: string;
  paragraphs: string[];
}

export interface HelpArticle {
  id: string;
  title: string;
  summary: string;
  /** Route patterns the article describes, such as `/channels/:id`. */
  routes: string[];
  sections: HelpSection[];
}

// One JSON file per page in `console/help/`, read at build time.
const modules = import.meta.glob<HelpArticle>("../../../help/*.json", {
  eager: true,
  import: "default",
});

export const articles: readonly HelpArticle[] = Object.values(modules).sort((a, b) =>
  a.title.localeCompare(b.title),
);

/** The article for a location: an exact route first, then the first matching pattern. */
export function articleForPath(pathname: string): HelpArticle | undefined {
  return (
    articles.find((article) => article.routes.includes(pathname)) ??
    articles.find((article) =>
      article.routes.some((route) => matchPath({ path: route, end: true }, pathname) !== null),
    )
  );
}
