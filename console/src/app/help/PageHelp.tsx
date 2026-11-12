import { X } from "lucide-react";
import { useId, useLayoutEffect, useRef, useState } from "react";
import { useLocation } from "react-router";
import { Button } from "@/components/Button";
import { articleForPath, articles } from "./articles";
import styles from "./PageHelp.module.css";

interface PageHelpProps {
  onClose: () => void;
}

/**
 * The help drawer: the topic for the current page, with a way to browse every topic.
 * Navigating to another page returns the drawer to that page's topic.
 */
export function PageHelp({ onClose }: PageHelpProps) {
  const { pathname } = useLocation();
  const current = articleForPath(pathname);
  const [shownPath, setShownPath] = useState(pathname);
  const [selectedId, setSelectedId] = useState(current?.id);
  const [browse, setBrowse] = useState(false);
  if (shownPath !== pathname) {
    setShownPath(pathname);
    setSelectedId(current?.id);
    setBrowse(false);
  }
  const article = articles.find((item) => item.id === selectedId);
  const heading = useRef<HTMLHeadingElement>(null);
  const titleId = useId();
  // Opening help moves focus into it; closing returns focus through the shell's trigger.
  useLayoutEffect(() => heading.current?.focus(), []);
  return (
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: Escape closes the drawer from anywhere inside it.
    <section
      aria-labelledby={titleId}
      className={styles.panel}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || event.defaultPrevented) return;
        event.preventDefault();
        onClose();
      }}
    >
      <header className={styles.header}>
        <h2 id={titleId} ref={heading} tabIndex={-1}>
          Page help
        </h2>
        <Button variant="quiet" aria-label="Close page help" onPress={onClose}>
          <X aria-hidden="true" />
        </Button>
      </header>
      <div className={styles.tools}>
        {/* Neither button disables itself: a button that goes inert while focused drops focus. */}
        <Button
          variant="quiet"
          onPress={() => {
            setSelectedId(current?.id);
            setBrowse(false);
          }}
        >
          Current page
        </Button>
        <Button variant="quiet" onPress={() => setBrowse(true)}>
          Browse all topics
        </Button>
      </div>
      <div className={styles.body}>
        {browse || article === undefined ? (
          <HelpTopics
            onOpen={(id) => {
              setSelectedId(id);
              setBrowse(false);
            }}
          />
        ) : (
          <HelpArticleView article={article} />
        )}
      </div>
    </section>
  );
}

export function HelpArticleView({ article }: { article: (typeof articles)[number] }) {
  return (
    <article className={styles.article}>
      <h3>{article.title}</h3>
      <p>{article.summary}</p>
      {article.sections.map((section) => (
        <section key={section.title}>
          <h4>{section.title}</h4>
          {section.paragraphs.map((paragraph) => (
            <p key={paragraph}>{paragraph}</p>
          ))}
        </section>
      ))}
    </article>
  );
}

function HelpTopics({ onOpen }: { onOpen: (id: string) => void }) {
  return (
    <div className={styles.topics}>
      <h3>Help topics</h3>
      <ul className={styles.topicList}>
        {articles.map((article) => (
          <li key={article.id}>
            <Button variant="quiet" onPress={() => onOpen(article.id)}>
              {article.title}
            </Button>
            <span className={styles.summary}>{article.summary}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
