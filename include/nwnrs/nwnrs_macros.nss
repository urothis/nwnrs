/// @file nwnrs_macros.nss
/// @brief Compiler-only project macros shipped by the nwnrs include package.

/// Builds the generated project event dispatcher from every annotated source.
/// The project macro is the sole owner of event registration grammar,
/// relationship validation, stable lane ordering, subscription deduplication,
/// and dispatcher generation.
/// @param input Compiler-provided project token stream containing
/// __nwnrs_event catalog records followed by __nwnrs_source records.
/// @return Private structured event-project plan and dispatcher envelope.
/// @private
proc_macro! nwnrs::__build_event_dispatcher {
    /// Executes the event-dispatcher compiler macro for one project token stream.
    /// @param input Compiler-provided project token stream.
    /// @return Private structured event-project plan and dispatcher envelope.
    /// @private
    tokenstream __build_event_dispatcher(tokenstream input)
    {
        tokenstream_list includes = __NWNRS_TokenStreamListNew();
        tokenstream_list registration_subjects = __NWNRS_TokenStreamListNew();
        tokenstream_list registration_phases = __NWNRS_TokenStreamListNew();
        tokenstream_list registration_handlers = __NWNRS_TokenStreamListNew();
        tokenstream_list registration_befores = __NWNRS_TokenStreamListNew();
        tokenstream_list registration_afters = __NWNRS_TokenStreamListNew();
        tokenstream_list event_subjects = __NWNRS_TokenStreamListNew();
        tokenstream_list event_names = __NWNRS_TokenStreamListNew();
        tokenstream_list event_phases = __NWNRS_TokenStreamListNew();
        ordering_graph handlers = __NWNRS_OrderingGraphNew();
        ordering_graph subscriptions = __NWNRS_OrderingGraphNew();
        tokenstream recovery_diagnostics = __NWNRS_QuoteEmpty();
        token_cursor project = __NWNRS_TokenCursorNew(input);
        int source_order = 0;
        int editor_recovery =
            __NWNRS_TokenCursorConsume(project, "__nwnrs_editor_recovery");
        tokenstream recovery_target = __NWNRS_QuoteEmpty();
        if (editor_recovery)
        {
            recovery_target = __NWNRS_TokenCursorNext(project);
        }
        int recovery_used = FALSE;

        while (__NWNRS_TokenCursorConsume(project, "__nwnrs_event"))
        {
            event_subjects = __NWNRS_TokenStreamListPush(
                event_subjects,
                __NWNRS_TokenCursorParseLiteral(project)
            );
            event_names = __NWNRS_TokenStreamListPush(
                event_names,
                __NWNRS_TokenCursorParseLiteral(project)
            );
            event_phases = __NWNRS_TokenStreamListPush(
                event_phases,
                __NWNRS_TokenCursorParseLiteral(project)
            );
        }

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

            token_cursor source = __NWNRS_TokenCursorNew(__NWNRS_TokenGroupContents(source_group));
            int source_has_handler = FALSE;
            while (!__NWNRS_TokenCursorIsEnd(source))
            {
                if (__NWNRS_TokenText(__NWNRS_TokenCursorPeek(source)) != "#")
                {
                    __NWNRS_TokenCursorNext(source);
                }
                else
                {
                    tokenstream_list pending_subjects = __NWNRS_TokenStreamListNew();
                    tokenstream_list pending_phases = __NWNRS_TokenStreamListNew();
                    tokenstream_list pending_befores = __NWNRS_TokenStreamListNew();
                    tokenstream_list pending_afters = __NWNRS_TokenStreamListNew();
                    tokenstream_list pending_attribute_hashes =
                        __NWNRS_TokenStreamListNew();
                    tokenstream_list pending_attributes = __NWNRS_TokenStreamListNew();
                    int collecting_attributes = TRUE;
                    while (collecting_attributes
                        && !__NWNRS_TokenCursorIsEnd(source)
                        && __NWNRS_TokenText(__NWNRS_TokenCursorPeek(source)) == "#")
                    {
                        tokenstream attribute_hash = __NWNRS_TokenCursorPeek(source);
                        __NWNRS_TokenCursorExpect(source, "#");
                        tokenstream attribute_group = __NWNRS_TokenCursorParseTree(source);
                        if (!__NWNRS_TokenIsGroup(attribute_group)
                            || __NWNRS_TokenDelimiter(attribute_group) != 2)
                        {
                            collecting_attributes = FALSE;
                        }
                        else
                        {
                            token_cursor attribute = __NWNRS_TokenCursorNew(
                                __NWNRS_TokenGroupContents(attribute_group)
                            );
                            __NWNRS_TokenCursorExpect(attribute, "nwnrs");
                            __NWNRS_TokenCursorExpect(attribute, ":");
                            __NWNRS_TokenCursorExpect(attribute, ":");
                            __NWNRS_TokenCursorExpect(attribute, "event");
                            __NWNRS_TokenCursorExpect(attribute, ":");
                            __NWNRS_TokenCursorExpect(attribute, ":");
                            tokenstream subject = __NWNRS_TokenCursorParseIdentifier(attribute);
                            __NWNRS_TokenCursorExpect(attribute, ":");
                            __NWNRS_TokenCursorExpect(attribute, ":");
                            tokenstream phase = __NWNRS_TokenCursorParseIdentifier(attribute);
                            tokenstream_list before_targets = __NWNRS_TokenStreamListNew();
                            tokenstream_list after_targets = __NWNRS_TokenStreamListNew();

                            if (!__NWNRS_TokenCursorIsEnd(attribute))
                            {
                                tokenstream relationship_group =
                                    __NWNRS_TokenCursorParseTree(attribute);
                                if (!__NWNRS_TokenCursorIsEnd(attribute)
                                    || !__NWNRS_TokenIsGroup(relationship_group)
                                    || __NWNRS_TokenDelimiter(relationship_group) != 1)
                                {
                                    __NWNRS_MacroErrorAt(
                                        attribute_group,
                                        "event relationships must follow the phase in parentheses"
                                    );
                                }
                                token_cursor relationships = __NWNRS_TokenCursorNew(
                                    __NWNRS_TokenGroupContents(relationship_group)
                                );
                                if (__NWNRS_TokenCursorIsEnd(relationships))
                                {
                                    __NWNRS_MacroErrorAt(
                                        relationship_group,
                                        "event relationship list must not be empty"
                                    );
                                }
                                int has_before_clause = FALSE;
                                int has_after_clause = FALSE;
                                while (!__NWNRS_TokenCursorIsEnd(relationships))
                                {
                                    tokenstream modifier =
                                        __NWNRS_TokenCursorParseIdentifier(relationships);
                                    tokenstream targets_group =
                                        __NWNRS_TokenCursorParseTree(relationships);
                                    if (!__NWNRS_TokenIsGroup(targets_group)
                                        || __NWNRS_TokenDelimiter(targets_group) != 1)
                                    {
                                        __NWNRS_MacroErrorAt(
                                            targets_group,
                                            "event relationship clause must use parentheses"
                                        );
                                    }
                                    token_cursor targets_cursor = __NWNRS_TokenCursorNew(
                                        __NWNRS_TokenGroupContents(targets_group)
                                    );
                                    tokenstream_list targets =
                                        __NWNRS_TokenCursorParseSeparated(
                                            targets_cursor,
                                            "identifier",
                                            ",",
                                            1,
                                            0
                                        );
                                    if (__NWNRS_TokenText(modifier) == "before")
                                    {
                                        if (has_before_clause)
                                        {
                                            __NWNRS_MacroErrorAt(
                                                modifier,
                                                "event attribute may contain at most one before clause"
                                            );
                                        }
                                        has_before_clause = TRUE;
                                        before_targets = targets;
                                    }
                                    else if (__NWNRS_TokenText(modifier) == "after")
                                    {
                                        if (has_after_clause)
                                        {
                                            __NWNRS_MacroErrorAt(
                                                modifier,
                                                "event attribute may contain at most one after clause"
                                            );
                                        }
                                        has_after_clause = TRUE;
                                        after_targets = targets;
                                    }
                                    else
                                    {
                                        __NWNRS_MacroErrorAt(
                                            modifier,
                                            "unknown event relationship modifier; expected before or after"
                                        );
                                    }
                                    if (!__NWNRS_TokenCursorIsEnd(relationships))
                                    {
                                        __NWNRS_TokenCursorExpect(relationships, ",");
                                        if (__NWNRS_TokenCursorIsEnd(relationships))
                                        {
                                            __NWNRS_MacroErrorAt(
                                                relationship_group,
                                                "event relationship list must not have a trailing comma"
                                            );
                                        }
                                    }
                                }
                            }

                            pending_subjects =
                                __NWNRS_TokenStreamListPush(pending_subjects, subject);
                            pending_phases =
                                __NWNRS_TokenStreamListPush(pending_phases, phase);
                            pending_befores =
                                __NWNRS_TokenStreamListPushList(pending_befores, before_targets);
                            pending_afters =
                                __NWNRS_TokenStreamListPushList(pending_afters, after_targets);
                            pending_attribute_hashes =
                                __NWNRS_TokenStreamListPush(
                                    pending_attribute_hashes,
                                    attribute_hash
                                );
                            pending_attributes =
                                __NWNRS_TokenStreamListPush(
                                    pending_attributes,
                                    attribute_group
                                );

                            if (__NWNRS_TokenCursorIsEnd(source)
                                || __NWNRS_TokenText(__NWNRS_TokenCursorPeek(source)) != "#")
                            {
                                collecting_attributes = FALSE;
                            }
                        }
                    }

                    if (__NWNRS_TokenStreamListLength(pending_subjects) > 0)
                    {
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

                        int pending_index = 0;
                        while (pending_index
                            < __NWNRS_TokenStreamListLength(pending_subjects))
                        {
                            tokenstream subject = __NWNRS_TokenStreamListGet(
                                pending_subjects,
                                pending_index
                            );
                            tokenstream phase = __NWNRS_TokenStreamListGet(
                                pending_phases,
                                pending_index
                            );
                            tokenstream_list before_targets =
                                __NWNRS_TokenStreamListGetList(
                                    pending_befores,
                                    pending_index
                                );
                            tokenstream_list after_targets =
                                __NWNRS_TokenStreamListGetList(
                                    pending_afters,
                                    pending_index
                                );
                            tokenstream attribute_group =
                                __NWNRS_TokenStreamListGet(
                                    pending_attributes,
                                    pending_index
                                );
                            tokenstream attribute_hash =
                                __NWNRS_TokenStreamListGet(
                                    pending_attribute_hashes,
                                    pending_index
                                );
                            string subject_text = __NWNRS_TokenText(subject);
                            string phase_text = __NWNRS_TokenText(phase);
                            string handler_text = __NWNRS_TokenText(handler);
                            string registration_key =
                                subject_text + "|" + phase_text + "|" + handler_text;

                            int existing_index = 0;
                            while (existing_index
                                < __NWNRS_TokenStreamListLength(registration_handlers))
                            {
                                tokenstream existing_subject =
                                    __NWNRS_TokenStreamListGet(
                                        registration_subjects,
                                        existing_index
                                    );
                                tokenstream existing_phase =
                                    __NWNRS_TokenStreamListGet(
                                        registration_phases,
                                        existing_index
                                    );
                                tokenstream existing_handler =
                                    __NWNRS_TokenStreamListGet(
                                        registration_handlers,
                                        existing_index
                                    );
                                if (__NWNRS_TokenText(existing_subject) == subject_text
                                    && __NWNRS_TokenText(existing_phase) == phase_text
                                    && __NWNRS_TokenText(existing_handler) == handler_text)
                                {
                                    __NWNRS_MacroErrorAt(
                                        handler,
                                        "function is registered more than once in the same event lane"
                                    );
                                }
                                existing_index += 1;
                            }

                            tokenstream runtime_subject;
                            tokenstream runtime_name;
                            tokenstream runtime_phase;
                            int event_supported = FALSE;
                            int subject_supported = FALSE;
                            int event_index = 0;
                            while (event_index
                                < __NWNRS_TokenStreamListLength(event_subjects))
                            {
                                tokenstream candidate_subject =
                                    __NWNRS_TokenStreamListGet(event_subjects, event_index);
                                tokenstream candidate_phase =
                                    __NWNRS_TokenStreamListGet(event_phases, event_index);
                                if (__NWNRS_TokenText(candidate_subject) == subject_text)
                                {
                                    subject_supported = TRUE;
                                    if (__NWNRS_TokenText(candidate_phase) == phase_text)
                                    {
                                        runtime_subject = candidate_subject;
                                        runtime_name =
                                            __NWNRS_TokenStreamListGet(event_names, event_index);
                                        runtime_phase = candidate_phase;
                                        event_supported = TRUE;
                                    }
                                }
                                event_index += 1;
                            }
                            if (!event_supported)
                            {
                                if (subject_supported)
                                {
                                    __NWNRS_MacroErrorAt(
                                        phase,
                                        "unsupported phase for nwnrs event subject"
                                    );
                                }
                                __NWNRS_MacroErrorAt(
                                    subject,
                                    "unsupported nwnrs event subject"
                                );
                            }

                            int target_index = 0;
                            while (target_index
                                < __NWNRS_TokenStreamListLength(before_targets))
                            {
                                tokenstream target =
                                    __NWNRS_TokenStreamListGet(before_targets, target_index);
                                int is_recovery_target = editor_recovery
                                    && __NWNRS_TokenSpanEquals(target, recovery_target);
                                if (is_recovery_target)
                                {
                                    recovery_used = TRUE;
                                    recovery_diagnostics = quote! {
                                        $recovery_diagnostics
                                        __nwnrs_diagnostic
                                            "incomplete event relationship target"
                                            $target
                                    };
                                }
                                if (!is_recovery_target
                                    && __NWNRS_TokenText(target) == handler_text)
                                {
                                    __NWNRS_MacroErrorAt(
                                        target,
                                        "event handler cannot be ordered relative to itself"
                                    );
                                }
                                int duplicate_index = 0;
                                while (!is_recovery_target && duplicate_index < target_index)
                                {
                                    if (__NWNRS_TokenText(
                                            __NWNRS_TokenStreamListGet(
                                                before_targets,
                                                duplicate_index
                                            )
                                        ) == __NWNRS_TokenText(target))
                                    {
                                        __NWNRS_MacroErrorAt(
                                            target,
                                            "duplicate target in event before clause"
                                        );
                                    }
                                    duplicate_index += 1;
                                }
                                target_index += 1;
                            }
                            target_index = 0;
                            while (target_index
                                < __NWNRS_TokenStreamListLength(after_targets))
                            {
                                tokenstream target =
                                    __NWNRS_TokenStreamListGet(after_targets, target_index);
                                int is_recovery_target = editor_recovery
                                    && __NWNRS_TokenSpanEquals(target, recovery_target);
                                if (is_recovery_target)
                                {
                                    recovery_used = TRUE;
                                    recovery_diagnostics = quote! {
                                        $recovery_diagnostics
                                        __nwnrs_diagnostic
                                            "incomplete event relationship target"
                                            $target
                                    };
                                }
                                if (!is_recovery_target
                                    && __NWNRS_TokenText(target) == handler_text)
                                {
                                    __NWNRS_MacroErrorAt(
                                        target,
                                        "event handler cannot be ordered relative to itself"
                                    );
                                }
                                int duplicate_index = 0;
                                while (!is_recovery_target && duplicate_index < target_index)
                                {
                                    if (__NWNRS_TokenText(
                                            __NWNRS_TokenStreamListGet(
                                                after_targets,
                                                duplicate_index
                                            )
                                        ) == __NWNRS_TokenText(target))
                                    {
                                        __NWNRS_MacroErrorAt(
                                            target,
                                            "duplicate target in event after clause"
                                        );
                                    }
                                    duplicate_index += 1;
                                }
                                duplicate_index = 0;
                                while (!is_recovery_target
                                    && duplicate_index
                                    < __NWNRS_TokenStreamListLength(before_targets))
                                {
                                    if (__NWNRS_TokenText(
                                            __NWNRS_TokenStreamListGet(
                                                before_targets,
                                                duplicate_index
                                            )
                                        ) == __NWNRS_TokenText(target))
                                    {
                                        __NWNRS_MacroErrorAt(
                                            target,
                                            "contradictory before and after relationship"
                                        );
                                    }
                                    duplicate_index += 1;
                                }
                                target_index += 1;
                            }

                            tokenstream dispatcher = quote! {
                                __nwnrs_ordered_handler {
                                    __nwnrs_registration
                                        $runtime_subject
                                        $runtime_name
                                        $runtime_phase
                                        $subject
                                        $phase
                                        $handler
                                        $source_name
                                        $attribute_hash
                                        $attribute_group
                                        $parameters
                                        { $($before_targets)* }
                                        { $($after_targets)* }
                                    __nwnrs_dispatch {
                                        if (sEventName == $runtime_name
                                            && sEventPhase == $runtime_phase)
                                        {
                                            $handler(jEvent);
                                        }
                                    }
                                }
                            };
                            __NWNRS_OrderingGraphAddNode(
                                handlers,
                                registration_key,
                                dispatcher,
                                handler,
                                source_order
                            );
                            source_order += 1;

                            string lane_key = subject_text + "|" + phase_text;
                            if (!__NWNRS_OrderingGraphContains(subscriptions, lane_key))
                            {
                                tokenstream subscription = quote! {
                                    _NWNRS_SubscribeEvent(
                                        $runtime_subject,
                                        $runtime_phase
                                    );
                                };
                                __NWNRS_OrderingGraphAddNode(
                                    subscriptions,
                                    lane_key,
                                    subscription,
                                    subject,
                                    source_order
                                );
                            }

                            registration_subjects = __NWNRS_TokenStreamListPush(
                                registration_subjects,
                                subject
                            );
                            registration_phases = __NWNRS_TokenStreamListPush(
                                registration_phases,
                                phase
                            );
                            registration_handlers = __NWNRS_TokenStreamListPush(
                                registration_handlers,
                                handler
                            );
                            registration_befores = __NWNRS_TokenStreamListPushList(
                                registration_befores,
                                before_targets
                            );
                            registration_afters = __NWNRS_TokenStreamListPushList(
                                registration_afters,
                                after_targets
                            );
                            pending_index += 1;
                        }
                        source_has_handler = TRUE;
                    }
                }
            }

            if (source_has_handler)
            {
                tokenstream include_line = quote! { #include $source_name };
                includes = __NWNRS_TokenStreamListPush(includes, include_line);
            }
        }

        int registration_index = 0;
        while (registration_index
            < __NWNRS_TokenStreamListLength(registration_handlers))
        {
            tokenstream subject =
                __NWNRS_TokenStreamListGet(registration_subjects, registration_index);
            tokenstream phase =
                __NWNRS_TokenStreamListGet(registration_phases, registration_index);
            tokenstream handler =
                __NWNRS_TokenStreamListGet(registration_handlers, registration_index);
            string subject_text = __NWNRS_TokenText(subject);
            string phase_text = __NWNRS_TokenText(phase);
            string handler_text = __NWNRS_TokenText(handler);
            string registration_key =
                subject_text + "|" + phase_text + "|" + handler_text;
            tokenstream_list before_targets =
                __NWNRS_TokenStreamListGetList(registration_befores, registration_index);
            tokenstream_list after_targets =
                __NWNRS_TokenStreamListGetList(registration_afters, registration_index);

            int relation_kind = 0;
            while (relation_kind < 2)
            {
                tokenstream_list targets = before_targets;
                if (relation_kind == 1)
                {
                    targets = after_targets;
                }
                int target_index = 0;
                while (target_index < __NWNRS_TokenStreamListLength(targets))
                {
                    tokenstream target =
                        __NWNRS_TokenStreamListGet(targets, target_index);
                    string target_text = __NWNRS_TokenText(target);
                    if (editor_recovery
                        && __NWNRS_TokenSpanEquals(target, recovery_target))
                    {
                        target_index += 1;
                    }
                    else
                    {
                    string target_key =
                        subject_text + "|" + phase_text + "|" + target_text;
                    if (!__NWNRS_OrderingGraphContains(handlers, target_key))
                    {
                        int handler_exists_elsewhere = FALSE;
                        int candidate_index = 0;
                        while (candidate_index
                            < __NWNRS_TokenStreamListLength(registration_handlers))
                        {
                            if (__NWNRS_TokenText(
                                    __NWNRS_TokenStreamListGet(
                                        registration_handlers,
                                        candidate_index
                                    )
                                ) == target_text)
                            {
                                handler_exists_elsewhere = TRUE;
                            }
                            candidate_index += 1;
                        }
                        if (handler_exists_elsewhere)
                        {
                            __NWNRS_MacroErrorAt(
                                target,
                                "event relationship target is registered only in another subject or phase"
                            );
                        }
                        __NWNRS_MacroErrorAt(
                            target,
                            "event relationship target is not registered: " + target_text
                        );
                    }

                    int edge_status;
                    if (relation_kind == 0)
                    {
                        edge_status = __NWNRS_OrderingGraphAddEdge(
                            handlers,
                            registration_key,
                            target_key
                        );
                    }
                    else
                    {
                        edge_status = __NWNRS_OrderingGraphAddEdge(
                            handlers,
                            target_key,
                            registration_key
                        );
                    }
                    if (edge_status == 0)
                    {
                        __NWNRS_MacroErrorAt(target, "duplicate event ordering edge");
                    }
                    if (edge_status < 0)
                    {
                        __NWNRS_MacroErrorAt(target, "contradictory event ordering edge");
                    }
                    target_index += 1;
                    }
                }
                relation_kind += 1;
            }
            registration_index += 1;
        }

        tokenstream_list cycle = __NWNRS_OrderingGraphCycle(handlers);
        if (__NWNRS_TokenStreamListLength(cycle) > 0)
        {
            string cycle_message = "event ordering cycle: ";
            int cycle_index = 0;
            while (cycle_index < __NWNRS_TokenStreamListLength(cycle))
            {
                if (cycle_index > 0)
                {
                    cycle_message += " -> ";
                }
                cycle_message += __NWNRS_TokenText(
                    __NWNRS_TokenStreamListGet(cycle, cycle_index)
                );
                cycle_index += 1;
            }
            __NWNRS_MacroErrorAt(
                __NWNRS_TokenStreamListGet(cycle, 0),
                cycle_message
            );
        }

        tokenstream_list dispatchers = __NWNRS_OrderingGraphSort(handlers);
        tokenstream_list subscription_calls = __NWNRS_OrderingGraphSort(subscriptions);
        tokenstream subscription_helper = __NWNRS_QuoteEmpty();
        tokenstream event_setup = __NWNRS_QuoteEmpty();
        tokenstream event_dispatch = __NWNRS_QuoteEmpty();
        if (__NWNRS_TokenStreamListLength(dispatchers) > 0)
        {
            subscription_helper = quote! {
                void _NWNRS_SubscribeEvent(string sEvent, string sPhase)
                {
                    NWNXPushString(sPhase);
                    NWNXPushString(sEvent);
                    NWNXCall("NWNRS", "SubscribeEvent");
                }
            };
            event_setup = quote! {
                NWNXCall("NWNRS", "GetCurrentEvent");
                json jEvent = JsonParse(NWNXPopString());
                string sEventName = JsonGetString(JsonObjectGet(jEvent, "name"));
                string sEventPhase = JsonGetString(JsonObjectGet(jEvent, "phase"));
                if (sEventName == "module.load") { $($subscription_calls)* }
            };
            event_dispatch = quote! { $($dispatchers)* };
        }

        if (recovery_used)
        {
            return quote! {
                __nwnrs_event_project {
                    __nwnrs_includes { $($includes)* }
                    __nwnrs_subscription_helper { $subscription_helper }
                    __nwnrs_event_setup { $event_setup }
                    __nwnrs_ordered_handlers { $event_dispatch }
                    __nwnrs_complete { FALSE }
                    __nwnrs_diagnostics { $recovery_diagnostics }
                }
            };
        }
        return quote! {
            __nwnrs_event_project {
                __nwnrs_includes { $($includes)* }
                __nwnrs_subscription_helper { $subscription_helper }
                __nwnrs_event_setup { $event_setup }
                __nwnrs_ordered_handlers { $event_dispatch }
                __nwnrs_complete { TRUE }
                __nwnrs_diagnostics {}
            }
        };
    }
}
