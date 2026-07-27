import type { NativeEventCatalogEntry } from './generated/native';

export interface EventCatalogDocument {
  readonly contents: string;
  readonly routeLines: ReadonlyMap<string, number>;
}

export function eventRouteKey(subject: string, phase: string): string {
  return `${subject}::${phase}`;
}

export function eventHoverMarkdown(event: NativeEventCatalogEntry): string {
  const lines = [
    `Runtime event: \`${event.name}\``,
    '',
    `Subject: \`${event.subject}\`  `,
    `Phase: \`${event.phase}\`  `,
    `Control: ${event.control}  `,
    `Skippable: ${event.skippable ? 'yes' : 'no'}  `,
    `Result: ${event.resultKind}  `,
    `Source: ${event.source}  `,
    `Frequency: ${event.frequency}  `,
    `Filter: ${event.filter}`,
  ];
  return lines.join('\n');
}

export function buildEventCatalogDocument(
  events: readonly NativeEventCatalogEntry[],
): EventCatalogDocument {
  const lines = [
    '# nwnrs event catalog',
    '',
    'This read-only document is generated from the runtime event catalog.',
    'Register a route with `#[nwnrs::event::<subject>::<phase>]`.',
    '',
  ];
  const routeLines = new Map<string, number>();
  for (const event of [...events].sort((left, right) =>
    eventRouteKey(left.subject, left.phase)
      .localeCompare(eventRouteKey(right.subject, right.phase)))) {
    const route = eventRouteKey(event.subject, event.phase);
    routeLines.set(route, lines.length);
    lines.push(`## ${route}`);
    lines.push('');
    lines.push('```nwscript');
    lines.push(`#[nwnrs::event::${route}]`);
    lines.push('void Handler(json event)');
    lines.push('{');
    lines.push('}');
    lines.push('```');
    lines.push('');
    lines.push(eventHoverMarkdown(event));
    lines.push('');
  }
  return {
    contents: `${lines.join('\n')}\n`,
    routeLines,
  };
}
