#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDir, "..");
const specsRoot = path.join(repositoryRoot, "doc", "specs");
const indexPath = path.join(specsRoot, "README.md");
const templatePath = path.join(specsRoot, "_template", "spec.md");

const allowedStatuses = new Set([
  "Exploration",
  "Draft",
  "Proposed",
  "Accepted",
  "Deprecated",
  "Rejected",
]);

const specIdPattern = /^[A-Z]{2,6}-\d{3}$/;
const headingPattern = /^# ([A-Z]{2,6}-\d{3}) — (.+)$/m;
const normativePattern =
  /\b(DOIT|DOIVENT|NE DOIT PAS|NE DOIVENT PAS|DEVRAIT|DEVRAIENT|NE DEVRAIT PAS|NE DEVRAIENT PAS|PEUT|PEUVENT)\b/;
const forbiddenPlaceholderPattern = /\b(TODO|TBD|FIXME|XXX)\b/;
const requiredSections = [
  "Objet",
  "Non-objectifs",
  "Spécification normative",
  "Diagnostics et erreurs",
  "Sécurité, confidentialité et ressources",
  "Interactions",
  "Compatibilité et migration",
  "Tests de conformité",
  "Questions ouvertes",
];
const optionalSections = new Set(["Alternatives rejetées"]);

function walk(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(entryPath) : [entryPath];
  });
}

function relative(filePath) {
  return path.relative(repositoryRoot, filePath);
}

function sectionBody(markdown, heading) {
  const escaped = heading.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = markdown.match(
    new RegExp(`^## ${escaped}\\s*\\n([\\s\\S]*?)(?=^## |\\z)`, "m"),
  );
  return match?.[1].trim() ?? "";
}

function metadata(markdown, label) {
  const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = markdown.match(
    new RegExp(`^- ${escaped} : (?:\\*\\*)?([^*\\n]+?)(?:\\*\\*)?$`, "m"),
  );
  return match?.[1].trim();
}

const errors = [];

if (!fs.existsSync(templatePath)) {
  errors.push("Template absent: doc/specs/_template/spec.md");
}

if (!fs.existsSync(indexPath)) {
  errors.push("Index absent: doc/specs/README.md");
}

const markdownFiles = walk(specsRoot).filter((file) => file.endsWith(".md"));
const specFiles = markdownFiles.filter(
  (file) =>
    path.basename(file) === "spec.md" &&
    !file.startsWith(path.join(specsRoot, "_template")),
);
const index = fs.existsSync(indexPath) ? fs.readFileSync(indexPath, "utf8") : "";
const knownIds = new Map();

for (const file of specFiles) {
  const markdown = fs.readFileSync(file, "utf8");
  const featureDirectory = path.basename(path.dirname(file));
  const domainDirectory = path.basename(path.dirname(path.dirname(file)));
  const heading = markdown.match(headingPattern);

  if (!heading) {
    errors.push(`${relative(file)}: titre attendu « FEAT-000 — Titre »`);
    continue;
  }

  const [, id, title] = heading;
  if (!specIdPattern.test(id)) {
    errors.push(`${relative(file)}: identifiant invalide ${id}`);
  }
  if (knownIds.has(id)) {
    errors.push(
      `${relative(file)}: identifiant ${id} déjà utilisé par ${relative(knownIds.get(id))}`,
    );
  } else {
    knownIds.set(id, file);
  }
  if (!featureDirectory.startsWith(`${id}-`)) {
    errors.push(`${relative(file)}: le répertoire doit commencer par ${id}-`);
  }
  if (!title.trim()) {
    errors.push(`${relative(file)}: titre vide`);
  }

  const status = metadata(markdown, "Statut");
  const version = metadata(markdown, "Version");
  const domain = metadata(markdown, "Domaine")?.replaceAll("`", "");

  if (!allowedStatuses.has(status)) {
    errors.push(`${relative(file)}: statut absent ou invalide (${status ?? "absent"})`);
  }
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
    errors.push(`${relative(file)}: version SemVer absente ou invalide`);
  }
  if (domain !== domainDirectory) {
    errors.push(
      `${relative(file)}: domaine ${domain ?? "absent"} différent du répertoire ${domainDirectory}`,
    );
  }

  if (!sectionBody(markdown, "Objet")) {
    errors.push(`${relative(file)}: section Objet absente ou vide`);
  }

  const actualSections = [...markdown.matchAll(/^## (.+)$/gm)].map(
    (match) => match[1].trim(),
  );
  const sectionPositions = requiredSections.map((section) =>
    actualSections.indexOf(section),
  );

  for (let index = 0; index < requiredSections.length; index += 1) {
    if (sectionPositions[index] === -1) {
      errors.push(`${relative(file)}: section obligatoire absente (${requiredSections[index]})`);
    }
    if (
      index > 0 &&
      sectionPositions[index] !== -1 &&
      sectionPositions[index - 1] !== -1 &&
      sectionPositions[index] <= sectionPositions[index - 1]
    ) {
      errors.push(
        `${relative(file)}: section hors ordre (${requiredSections[index]})`,
      );
    }
  }

  for (const section of actualSections) {
    if (!requiredSections.includes(section) && !optionalSections.has(section)) {
      errors.push(`${relative(file)}: section H2 non prévue par le template (${section})`);
    }
  }

  const alternativesPosition = actualSections.indexOf("Alternatives rejetées");
  const testsPosition = actualSections.indexOf("Tests de conformité");
  const questionsPosition = actualSections.indexOf("Questions ouvertes");
  if (
    alternativesPosition !== -1 &&
    !(alternativesPosition > testsPosition && alternativesPosition < questionsPosition)
  ) {
    errors.push(`${relative(file)}: Alternatives rejetées doit précéder Questions ouvertes`);
  }
  if (!normativePattern.test(markdown)) {
    errors.push(`${relative(file)}: aucune exigence normative détectée`);
  }
  if (forbiddenPlaceholderPattern.test(markdown)) {
    errors.push(`${relative(file)}: marqueur temporaire interdit`);
  }
  if (!markdown.endsWith("\n")) {
    errors.push(`${relative(file)}: newline finale absente`);
  }

  const indexTarget = path
    .relative(specsRoot, file)
    .split(path.sep)
    .join("/");
  if (!index.includes(`(${indexTarget})`)) {
    errors.push(`${relative(file)}: spec absente de doc/specs/README.md`);
  }

}

for (const file of markdownFiles) {
  if (file === templatePath) continue;
  const markdown = fs.readFileSync(file, "utf8");

  for (const match of markdown.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
    const target = match[1];
    if (/^[a-z]+:\/\//i.test(target) || target.startsWith("#")) continue;
    const targetPath = path.resolve(path.dirname(file), target.split("#")[0]);
    if (!fs.existsSync(targetPath)) {
      errors.push(`${relative(file)}: lien cassé ${target}`);
    }
  }

  for (const match of markdown.matchAll(/\b([A-Z]{2,6}-\d{3})\b/g)) {
    if (!knownIds.has(match[1])) {
      errors.push(`${relative(file)}: référence inconnue ${match[1]}`);
    }
  }
}

if (errors.length > 0) {
  console.error(`Erreurs (${errors.length}) :`);
  for (const error of [...new Set(errors)]) console.error(`  - ${error}`);
  process.exit(1);
}

const totalLines = markdownFiles.reduce(
  (count, file) => count + fs.readFileSync(file, "utf8").split("\n").length - 1,
  0,
);

console.log(
  `OK — ${specFiles.length} specs conformes, ${knownIds.size} identifiants uniques, ${totalLines} lignes Markdown.`,
);
