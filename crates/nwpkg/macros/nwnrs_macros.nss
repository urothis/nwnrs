/// @file nwnrs_macros.nss
/// @brief Compiler-only project macros shipped by the nwnrs include package.

/// Builds the generated project event dispatcher from every annotated source.
/// Validates event identities and handler signatures, emits source includes,
/// registers supported subscriptions during module.load, and dispatches the
/// current JSON event to its matching handlers.
/// @param input Compiler-provided project token stream containing
/// __nwnrs_source records.
/// @return Generated NWScript main() dispatcher token stream.
/// @private
proc_macro! nwnrs::__build_event_dispatcher {
    /// Executes the event-dispatcher compiler macro for one project token stream.
    /// @param input Compiler-provided project token stream.
    /// @return Generated NWScript main() dispatcher token stream.
    /// @private
    tokenstream __build_event_dispatcher(tokenstream input)
    {
        tokenstream_list includes = __NWNRS_TokenStreamListNew();
        tokenstream_list handlers = __NWNRS_TokenStreamListNew();
        tokenstream_list dispatchers = __NWNRS_TokenStreamListNew();
        tokenstream_list subscriptions = __NWNRS_TokenStreamListNew();
        token_cursor project = __NWNRS_TokenCursorNew(input);

        while (!__NWNRS_TokenCursorIsEnd(project))
        {
            __NWNRS_TokenCursorExpect(project, "__nwnrs_source");
            tokenstream source_name = __NWNRS_TokenCursorParseLiteral(project);
            tokenstream source_group = __NWNRS_TokenCursorParseTree(project);
            if (!__NWNRS_TokenIsGroup(source_group)
                || __NWNRS_TokenDelimiter(source_group) != 3)
            {
                __NWNRS_MacroErrorAt(source_group, "nwnrs project source must be wrapped in braces");
            }

            tokenstream source_tokens = __NWNRS_TokenGroupContents(source_group);
            token_cursor source = __NWNRS_TokenCursorNew(source_tokens);
            int has_handler = FALSE;

            while (!__NWNRS_TokenCursorIsEnd(source))
            {
                if (__NWNRS_TokenCursorConsume(source, "#"))
                {
                    tokenstream attribute_group = __NWNRS_TokenCursorParseTree(source);
                    if (!__NWNRS_TokenIsGroup(attribute_group)
                        || __NWNRS_TokenDelimiter(attribute_group) != 2)
                    {
                        __NWNRS_MacroErrorAt(attribute_group, "nwnrs compiler attribute must use brackets");
                    }

                    token_cursor attribute = __NWNRS_TokenCursorNew(__NWNRS_TokenGroupContents(attribute_group));
                    __NWNRS_TokenCursorExpect(attribute, "nwnrs");
                    __NWNRS_TokenCursorExpect(attribute, ":");
                    __NWNRS_TokenCursorExpect(attribute, ":");
                    __NWNRS_TokenCursorExpect(attribute, "events");
                    tokenstream event_group = __NWNRS_TokenCursorParseTree(attribute);
                    if (!__NWNRS_TokenCursorIsEnd(attribute)
                        || !__NWNRS_TokenIsGroup(event_group)
                        || __NWNRS_TokenDelimiter(event_group) != 1)
                    {
                        __NWNRS_MacroErrorAt(attribute_group, "expected #[nwnrs::events(event_identity)]");
                    }

                    token_cursor event = __NWNRS_TokenCursorNew(__NWNRS_TokenGroupContents(event_group));
                    tokenstream event_identity = __NWNRS_TokenCursorParseIdentifier(event);
                    if (!__NWNRS_TokenCursorIsEnd(event))
                    {
                        __NWNRS_MacroErrorAt(event_group, "event identity must be one identifier");
                    }
                    string event_name = __NWNRS_TokenText(event_identity);

                    tokenstream return_type = __NWNRS_TokenCursorParseTree(source);
                    tokenstream handler = __NWNRS_TokenCursorParseIdentifier(source);
                    tokenstream parameters = __NWNRS_TokenCursorParseTree(source);
                    tokenstream body = __NWNRS_TokenCursorParseTree(source);
                    if (__NWNRS_TokenText(return_type) != "void"
                        || !__NWNRS_TokenIsGroup(parameters)
                        || __NWNRS_TokenDelimiter(parameters) != 1
                        || !__NWNRS_TokenIsGroup(body)
                        || __NWNRS_TokenDelimiter(body) != 3)
                    {
                        __NWNRS_MacroErrorAt(handler, "event handler must be void Handler(json event) { ... }");
                    }

                    token_cursor parameter = __NWNRS_TokenCursorNew(__NWNRS_TokenGroupContents(parameters));
                    if (__NWNRS_TokenCursorIsEnd(parameter))
                    {
                        __NWNRS_MacroErrorAt(parameters, "event handler must accept exactly one json parameter");
                    }
                    __NWNRS_TokenCursorExpect(parameter, "json");
                    __NWNRS_TokenCursorParseIdentifier(parameter);
                    if (!__NWNRS_TokenCursorIsEnd(parameter))
                    {
                        __NWNRS_MacroErrorAt(parameters, "event handler must accept exactly one json parameter");
                    }

                    int index = 0;
                    while (index < __NWNRS_TokenStreamListLength(handlers))
                    {
                        tokenstream existing = __NWNRS_TokenStreamListGet(handlers, index);
                        if (__NWNRS_TokenText(existing) == __NWNRS_TokenText(handler))
                        {
                            __NWNRS_MacroErrorAt(handler, "duplicate nwnrs event handler");
                        }
                        index += 1;
                    }
                    handlers = __NWNRS_TokenStreamListPush(handlers, handler);
                    tokenstream dispatcher;
                    tokenstream subscription;
                    // NWNRS_EVENT_CATALOG_BEGIN
                    if (FALSE)
                    {
                        dispatcher = __NWNRS_QuoteEmpty();
                        subscription = __NWNRS_QuoteEmpty();
                    }
                    // NWNRS_EVENT_CATALOG_END
                    else
                    {
                        __NWNRS_MacroErrorAt(event_identity, "unsupported nwnrs event identity");
                    }
                    dispatchers = __NWNRS_TokenStreamListPush(dispatchers, dispatcher);
                    subscriptions = __NWNRS_TokenStreamListPush(subscriptions, subscription);
                    has_handler = TRUE;
                }
                else
                {
                    __NWNRS_TokenCursorNext(source);
                }
            }

            if (has_handler)
            {
                tokenstream include_line = quote! { #include $source_name };
                includes = __NWNRS_TokenStreamListPush(includes, include_line);
            }
        }

        handlers = __NWNRS_TokenStreamListSort(handlers);
        dispatchers = __NWNRS_TokenStreamListSort(dispatchers);
        subscriptions = __NWNRS_TokenStreamListSort(subscriptions);
        tokenstream event_setup = __NWNRS_QuoteEmpty();
        tokenstream event_dispatch = __NWNRS_QuoteEmpty();
        if (__NWNRS_TokenStreamListLength(handlers) > 0)
        {
            event_setup = quote! {
                NWNXCall("NWNRS", "GetCurrentEvent");
                json jEvent = JsonParse(NWNXPopString());
                string sEventName = JsonGetString(JsonObjectGet(jEvent, "name"));
                string sEventPhase = JsonGetString(JsonObjectGet(jEvent, "phase"));
                if (sEventName == "module.load") { $($subscriptions)* }
            };
            event_dispatch = quote! { $($dispatchers)* };
        }

        return quote! {
            $($includes)*
            void main()
            {
                $event_setup
                $event_dispatch
            }
        };
    }
}
