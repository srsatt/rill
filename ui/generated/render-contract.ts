// @generated from rill-contracts by cargo xtask generate-contracts. Do not edit.

export type RenderMode = "modern" | "reader";

export type RenderRequest = { version: number, template: string, mode: RenderMode, locale: string, renderId: string, props: unknown, assets: { [key in string]: string }, csrfToken: string, };

export type RenderResponse = { version: number, status: number, headHtml: string, bodyHtml: string, hydrationState: unknown, };

export type StreamLink = { name: string, slug: string, };

export type StoryCardModel = { id: string, title: string, summary: string, source: string, curator: string | null, publishedAt: string, coverageCount: number, readingMinutes: number, tags: Array<string>, };

export type FeedPageModel = { title: string, activeStream: string, streams: Array<StreamLink>, stories: Array<StoryCardModel>, username: string, page: number, previousPage: number | null, nextPage: number | null, };

export type LibraryPageModel = { title: string, username: string, kind: string, query: string | null, stories: Array<StoryCardModel>, };

export type SourcesPageModel = { title: string, username: string, emailAvailable: boolean, telegramAvailable: boolean, };

export type CuratorPathModel = { kind: string, curatorId: string, sourceName: string | null, curatorCommentary: string | null, parentTitle: string | null, parentUrl: string | null, };

export type StoryLinkModel = { url: string, relation: string, title: string | null, };

export type StoryVariantModel = { documentId: string, title: string, summary: string, bodyText: string, canonicalUrl: string | null, links: Array<StoryLinkModel>, author: string | null, publisher: string | null, language: string | null, publishedAt: string | null, curators: Array<CuratorPathModel>, selected: boolean, };

export type StoryPageModel = { title: string, storyId: string, representative: StoryVariantModel, variants: Array<StoryVariantModel>, coverageCount: number, read: boolean, favorite: boolean, explicitFeedback: string | null, reader: boolean, };

export type ReaderPreferencesPageModel = { title: string, username: string, streams: Array<StreamLink>, activeStream: string, };

export type LoginPageModel = { title: string, error: string | null, };

export type ReaderPairPageModel = { title: string, error: string | null, };

export type ReaderDeviceModel = { id: string, label: string, createdAt: number, lastUsedAt: number, expiresAt: number, userAgent: string | null, };

export type ReaderSettingsPageModel = { title: string, username: string, devices: Array<ReaderDeviceModel>, newPairingCode: string | null, pairingExpiresAt: number | null, };

export type AdminPageModel = { title: string, username: string, };
