import { Link } from "react-aria-components";
import { useSearchParams } from "react-router";
import { articles } from "@/app/help/articles";
import { HelpArticleView } from "@/app/help/PageHelp";
import { Section } from "@/components/Section";
import styles from "./settings.module.css";

/** Every help topic: the list on one side, the chosen topic on the other (`?topic=`). */
export function HelpTab() {
  const [params] = useSearchParams();
  const selected = articles.find((article) => article.id === params.get("topic")) ?? articles[0];
  return (
    <Section title="Help topics">
      <div className={styles.help}>
        <nav aria-label="Help topics" className={styles.topics}>
          {articles.map((article) => (
            <Link
              key={article.id}
              href={`?topic=${article.id}`}
              className={styles.topic}
              aria-current={article.id === selected?.id ? "true" : undefined}
            >
              {article.title}
            </Link>
          ))}
        </nav>
        {selected && <HelpArticleView article={selected} />}
      </div>
    </Section>
  );
}
