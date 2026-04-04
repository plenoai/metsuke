# Metsuke — Project Conventions

## CSS Naming: Strict BEM

All CSS classes follow [BEM](https://getbem.com/) (Block Element Modifier).

### Syntax

```
.block
.block__element
.block--modifier
.block__element--modifier
```

- **Block**: standalone component (`header`, `repo-card`, `dash-card`, `pagination`)
- **Element**: part of a block, separated by `__` (`header__left`, `repo-card__name`)
- **Modifier**: variant/state of a block or element, separated by `--` (`badge--pass`, `dash-card--empty`)
- Multi-word blocks/elements use single hyphens: `page-header`, `repo-card`, `inline-row`

### State classes

JS-toggled states use `is-*` prefix as adjacent classes:

```html
<a class="nav__item is-active">
<button class="btn--verify is-running">
<div class="pr-item is-selected">
```

### Utility classes

Spacing and text utilities use `u-` prefix:

```css
.u-mt-0   /* margin-top: 0 */
.u-mt-md  /* margin-top: medium */
.u-mb-0   /* margin-bottom: 0 */
.u-text-muted
```

### Rules

1. Never style IDs (`#foo`). IDs are for JS targeting only.
2. Never reuse a component class (e.g. `btn--verify`) on unrelated elements. Use the component's own scoped styles.
3. Tag selectors inside blocks are acceptable for leaf nodes (`.findings__table th`).
4. Keep nesting flat: `block__element` only. Never `block__element__subelement`.
5. New CSS classes MUST follow this BEM convention.

### Block inventory

| Block | Purpose |
|-------|---------|
| `header` | Site header (brand, nav, user badge) |
| `nav` | Navigation links |
| `user-badge` | Logged-in user info + logout |
| `section` | Content section with title |
| `card` | Generic card container |
| `badge` | Status badge (--pass, --fail, --review, --na, --private) |
| `btn` | Button (--link, --verify) |
| `page-header` | Page title + toolbar row |
| `toolbar` | Horizontal control group |
| `inline-row` | Inline flex row for badges (--tight) |
| `pagination` | Pagination controls |
| `findings` | Findings table section |
| `summary` | Summary bar |
| `form-group` | Form field group |
| `kbd-help` | Keyboard shortcut overlay |
| `empty-state` | No-results placeholder |
| `skeleton` | Loading skeleton |
| `dashboard` | Dashboard grid |
| `dash-card` | Dashboard metric card |
| `activity` | Recent activity list |
| `repo-card` | Repository list card |
| `repo-grid` | Repository list container |
| `pr-item` | Pull request list item |
| `pr-detail` | PR expanded detail |
| `pr-state` | PR state badge (--open, --closed, --merged, --draft) |
| `release-item` | Release list item |
| `release-form` | Release verification form |
| `audit` | Audit log section |
| `type-badge` | Audit type indicator (--release, --pr, --repo) |
| `trigger-badge` | Audit trigger indicator (--manual, --webhook) |
| `install` | Settings installation item |
| `tag` | Label tag (--org) |
| `code-wrap` | Code block with copy button |
| `error-page` | Standalone error page |
| `error-inline` | JS-rendered inline error |
| `readme` | README display area |
| `search-bar` | Search input wrapper |
| `detail-heading` | Repo detail page heading |

## Stack

- Backend: Rust (Axum) + SQLite
- Frontend: Vanilla JS + Tera templates
- CSS: Single `style.css` + `themes.css` (variables)
- Testing: Playwright E2E (`e2e/`)
- Deploy: Fly.io (trunk-based, main push)
- Releases: tag push → trusted publishing
