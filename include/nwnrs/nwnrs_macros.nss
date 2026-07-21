/// @file nwnrs_macros.nss
/// @brief Compiler-only project macros shipped by the nwnrs include package.

proc_macro! nwnrs::__build_event_dispatcher {
    tokenstream __build_event_dispatcher(tokenstream input)
    {
        tokenstream_list includes = __NWNRS_TokenStreamListNew();
        tokenstream_list handlers = __NWNRS_TokenStreamListNew();
        tokenstream_list dispatchers = __NWNRS_TokenStreamListNew();
        token_cursor project = __NWNRS_TokenCursorNew(input);

        while (!__NWNRS_TokenCursorIsEnd(project))
        {
            __NWNRS_TokenCursorExpect(project, "__nwnrs_source");
            tokenstream source_name = __NWNRS_TokenCursorParseLiteral(project);
            tokenstream source_group = __NWNRS_TokenCursorParseTree(project);
            if (!__NWNRS_TokenIsGroup(source_group)
                || __NWNRS_TokenDelimiter(source_group) != 3)
            {
                __NWNRS_MacroErrorAt(
                    source_group,
                    "nwnrs project source must be wrapped in braces"
                );
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
                        __NWNRS_MacroErrorAt(
                            attribute_group,
                            "nwnrs compiler attribute must use brackets"
                        );
                    }

                    token_cursor attribute = __NWNRS_TokenCursorNew(
                        __NWNRS_TokenGroupContents(attribute_group)
                    );
                    __NWNRS_TokenCursorExpect(attribute, "nwnrs");
                    __NWNRS_TokenCursorExpect(attribute, ":");
                    __NWNRS_TokenCursorExpect(attribute, ":");
                    __NWNRS_TokenCursorExpect(attribute, "events");
                    tokenstream event_group = __NWNRS_TokenCursorParseTree(attribute);
                    if (!__NWNRS_TokenCursorIsEnd(attribute)
                        || !__NWNRS_TokenIsGroup(event_group)
                        || __NWNRS_TokenDelimiter(event_group) != 1)
                    {
                        __NWNRS_MacroErrorAt(
                            attribute_group,
                            "expected #[nwnrs::events(event_identity)]"
                        );
                    }

                    token_cursor event = __NWNRS_TokenCursorNew(
                        __NWNRS_TokenGroupContents(event_group)
                    );
                    tokenstream event_identity = __NWNRS_TokenCursorParseIdentifier(event);
                    if (!__NWNRS_TokenCursorIsEnd(event))
                    {
                        __NWNRS_MacroErrorAt(
                            event_group,
                            "event identity must be one identifier"
                        );
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
                        __NWNRS_MacroErrorAt(
                            handler,
                            "event handler must be void Handler(json event) { ... }"
                        );
                    }

                    token_cursor parameter = __NWNRS_TokenCursorNew(
                        __NWNRS_TokenGroupContents(parameters)
                    );
                    if (__NWNRS_TokenCursorIsEnd(parameter))
                    {
                        __NWNRS_MacroErrorAt(
                            parameters,
                            "event handler must accept exactly one json parameter"
                        );
                    }
                    __NWNRS_TokenCursorExpect(parameter, "json");
                    __NWNRS_TokenCursorParseIdentifier(parameter);
                    if (!__NWNRS_TokenCursorIsEnd(parameter))
                    {
                        __NWNRS_MacroErrorAt(
                            parameters,
                            "event handler must accept exactly one json parameter"
                        );
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
                    if (event_name == "module_load")
                    {
                        dispatcher = quote! {
                            if (sEventName == "module.load") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_add_before")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.add" && sEventPhase == "before") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_add_after")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.add" && sEventPhase == "after") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_remove_before")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.remove" && sEventPhase == "before") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_remove_after")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.remove" && sEventPhase == "after") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_possess_familiar_before")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.possess_familiar" && sEventPhase == "before") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_possess_familiar_after")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.possess_familiar" && sEventPhase == "after") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_unpossess_familiar_before")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.unpossess_familiar" && sEventPhase == "before") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "associate_unpossess_familiar_after")
                    {
                        dispatcher = quote! {
                            if (sEventName == "associate.unpossess_familiar" && sEventPhase == "after") { $handler(jEvent); }
                        };
                    }
                    else if (event_name == "object_lock_before")
                    {
                        dispatcher = quote! { if (sEventName == "object.lock" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "object_lock_after")
                    {
                        dispatcher = quote! { if (sEventName == "object.lock" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "object_unlock_before")
                    {
                        dispatcher = quote! { if (sEventName == "object.unlock" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "object_unlock_after")
                    {
                        dispatcher = quote! { if (sEventName == "object.unlock" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "object_use_before")
                    {
                        dispatcher = quote! { if (sEventName == "object.use" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "object_use_after")
                    {
                        dispatcher = quote! { if (sEventName == "object.use" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "placeable_open_before")
                    {
                        dispatcher = quote! { if (sEventName == "placeable.open" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "placeable_open_after")
                    {
                        dispatcher = quote! { if (sEventName == "placeable.open" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "placeable_close_before")
                    {
                        dispatcher = quote! { if (sEventName == "placeable.close" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "placeable_close_after")
                    {
                        dispatcher = quote! { if (sEventName == "placeable.close" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "inventory_add_gold_before")
                    {
                        dispatcher = quote! { if (sEventName == "inventory.add_gold" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "inventory_add_gold_after")
                    {
                        dispatcher = quote! { if (sEventName == "inventory.add_gold" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "inventory_remove_gold_before")
                    {
                        dispatcher = quote! { if (sEventName == "inventory.remove_gold" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "inventory_remove_gold_after")
                    {
                        dispatcher = quote! { if (sEventName == "inventory.remove_gold" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "feat_use_before")
                    {
                        dispatcher = quote! { if (sEventName == "feat.use" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "feat_use_after")
                    {
                        dispatcher = quote! { if (sEventName == "feat.use" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "journal_open_before")
                    {
                        dispatcher = quote! { if (sEventName == "journal.open" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "journal_open_after")
                    {
                        dispatcher = quote! { if (sEventName == "journal.open" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "journal_close_before")
                    {
                        dispatcher = quote! { if (sEventName == "journal.close" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "journal_close_after")
                    {
                        dispatcher = quote! { if (sEventName == "journal.close" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "timing_bar_start_before")
                    {
                        dispatcher = quote! { if (sEventName == "timing_bar.start" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "timing_bar_start_after")
                    {
                        dispatcher = quote! { if (sEventName == "timing_bar.start" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "timing_bar_stop_before")
                    {
                        dispatcher = quote! { if (sEventName == "timing_bar.stop" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "timing_bar_stop_after")
                    {
                        dispatcher = quote! { if (sEventName == "timing_bar.stop" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "timing_bar_cancel_before")
                    {
                        dispatcher = quote! { if (sEventName == "timing_bar.cancel" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "timing_bar_cancel_after")
                    {
                        dispatcher = quote! { if (sEventName == "timing_bar.cancel" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "object_broadcast_safe_projectile_before")
                    {
                        dispatcher = quote! { if (sEventName == "object.broadcast_safe_projectile" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "object_broadcast_safe_projectile_after")
                    {
                        dispatcher = quote! { if (sEventName == "object.broadcast_safe_projectile" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "skill_use_before")
                    {
                        dispatcher = quote! { if (sEventName == "skill.use" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "skill_use_after")
                    {
                        dispatcher = quote! { if (sEventName == "skill.use" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_use_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.use" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_use_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.use" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_inventory_open_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.inventory_open" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_inventory_open_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.inventory_open" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_inventory_close_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.inventory_close" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_inventory_close_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.inventory_close" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_scroll_learn_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.scroll_learn" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_scroll_learn_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.scroll_learn" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_use_lore_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.use_lore" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_use_lore_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.use_lore" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_pay_to_identify_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.pay_to_identify" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_pay_to_identify_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.pay_to_identify" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_destroy_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.destroy" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_destroy_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.destroy" && sEventPhase == "after") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_decrement_stack_size_before")
                    {
                        dispatcher = quote! { if (sEventName == "item.decrement_stack_size" && sEventPhase == "before") { $handler(jEvent); } };
                    }
                    else if (event_name == "item_decrement_stack_size_after")
                    {
                        dispatcher = quote! { if (sEventName == "item.decrement_stack_size" && sEventPhase == "after") { $handler(jEvent); } };
                    }

                    else
                    {
                        __NWNRS_MacroErrorAt(event_identity, "unsupported nwnrs event identity");
                    }
                    dispatchers = __NWNRS_TokenStreamListPush(dispatchers, dispatcher);
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
        tokenstream event_setup = __NWNRS_QuoteEmpty();
        tokenstream event_dispatch = __NWNRS_QuoteEmpty();
        if (__NWNRS_TokenStreamListLength(handlers) > 0)
        {
            event_setup = quote! {
                NWNXCall("NWNRS", "GetCurrentEvent");
                json jEvent = JsonParse(NWNXPopString());
                string sEventName = JsonGetString(JsonObjectGet(jEvent, "name"));
                string sEventPhase = JsonGetString(JsonObjectGet(jEvent, "phase"));
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
