import { ArrowLeft, BookOpen } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { TextInput } from "../components/Field";
import pageStyles from "../pages/Page.module.css";
import styles from "./HelpPage.module.css";
import { HELP_TOPICS, findTopic } from "./topics";

interface HelpPageProps {
  topic: string;
  onTopicChange: (topic: string) => void;
  onClose: () => void;
}

export function HelpPage({ topic, onTopicChange, onClose }: HelpPageProps) {
  const [filter, setFilter] = useState("");
  const contentRef = useRef<HTMLDivElement>(null);
  const current = findTopic(topic);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    contentRef.current?.scrollTo({ top: 0 });
  }, [topic]);

  const visible = useMemo(() => {
    const terms = filter.toLowerCase().split(/\s+/).filter(Boolean);
    return HELP_TOPICS.filter((entry) => {
      const haystack = `${entry.title} ${entry.summary} ${entry.keywords}`.toLowerCase();
      return terms.every((term) => haystack.includes(term));
    });
  }, [filter]);

  const others = HELP_TOPICS.filter((entry) => entry.id !== current.id).slice(0, 4);

  return (
    <div className={pageStyles.page} role="dialog" aria-label="Help">
      <nav className={pageStyles.sidebar}>
        <button type="button" className={pageStyles.back} onClick={onClose}>
          <ArrowLeft size={15} />
          Back
        </button>
        <span className={pageStyles.sidebarTitle}>Help</span>
        <div className={styles.search}>
          <TextInput value={filter} onValueChange={setFilter} placeholder="Search the help" aria-label="Search the help" />
        </div>
        {visible.map((entry) => (
          <button
            key={entry.id}
            type="button"
            className={pageStyles.navItem}
            aria-current={entry.id === current.id ? "page" : undefined}
            onClick={() => onTopicChange(entry.id)}
          >
            <BookOpen size={14} />
            {entry.title}
          </button>
        ))}
        {visible.length === 0 && <p className={styles.empty}>No topic matches.</p>}
      </nav>
      <div className={pageStyles.content} ref={contentRef}>
        <article className={pageStyles.inner}>
          <header className={pageStyles.heading}>
            <h1>{current.title}</h1>
            <p>{current.summary}</p>
          </header>
          <div className={styles.article}>{current.body}</div>
          <section className={pageStyles.group}>
            <h2 className={pageStyles.groupTitle}>More topics</h2>
            <div className={styles.related}>
              {others.map((entry) => (
                <button key={entry.id} type="button" onClick={() => onTopicChange(entry.id)}>
                  {entry.title}
                </button>
              ))}
            </div>
          </section>
        </article>
      </div>
    </div>
  );
}
