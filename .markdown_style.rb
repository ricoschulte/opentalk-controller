# SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
#
# SPDX-License-Identifier: EUPL-1.2

# Enable all rules by default
all

# All unordered lists must use '-' consistently at all levels
rule 'MD004', :style => :dash
rule 'MD007', :indent => 4

# Markdown tables cant have breaks in them thus this is set to the largest table.
rule 'MD013', :line_length => 790

rule 'MD029', :style => :ordered

# Disable duplicate heading check
exclude_rule 'MD024'

# Disable check that first line must be top level header as its incompatible with docusaurus
exclude_rule 'MD041'
