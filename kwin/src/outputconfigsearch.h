/*
    KWin - the KDE window manager
    This file is part of the KDE project.

    SPDX-License-Identifier: GPL-2.0-or-later
*/

#pragma once

#include "core/output.h"

#include <QSize>
#include <memory>
#include <optional>
#include <vector>

namespace KWin
{

class BackendOutput;

/**
 * Represents one candidate mode option for a single output.
 * A null mode means "disabled".
 */
struct OutputModeOption
{
    std::shared_ptr<OutputMode> mode; // nullptr = disabled
    uint32_t maxBpc = 0; // 0 = automatic/native
    uint64_t bandwidth() const;
};

/**
 * Represents one candidate configuration across all outputs.
 */
struct CandidateConfig
{
    // Per-output index into that output's mode option list
    std::vector<int> modeChoices;
    double score = 0;
};

namespace OutputConfigSearch
{

/**
 * Select representative modes for an output. For each resolution, keeps:
 * - Highest refresh rate
 * - Lowest refresh >= 110Hz ("120Hz regime")
 * - Lowest refresh >= 50Hz ("60Hz regime")
 * - The preferred/native mode
 * Plus a "disabled" option (nullptr mode) if totalOutputCount > 1.
 *
 * Also generates 8bpc variants of each enabled mode for bandwidth reduction.
 */
std::vector<OutputModeOption> pruneModes(BackendOutput *output, int totalOutputCount);

/**
 * Cross-product enumeration of all candidate configurations, excluding all-disabled.
 */
std::vector<CandidateConfig> enumerateCandidates(const std::vector<std::vector<OutputModeOption>> &perOutputOptions);

/**
 * Score each candidate. Higher is better.
 * - +10000 per enabled output
 * - +pixels/1000 for resolution
 * - +refreshRate_mHz/100 for refresh rate
 * - Small penalty for reduced bpc
 */
void scoreCandidates(std::vector<CandidateConfig> &candidates,
                     const std::vector<std::vector<OutputModeOption>> &perOutputOptions);

/**
 * Reorder candidates using greedy farthest-point sampling for space-filling coverage.
 * The first element is the highest-scored candidate. Each subsequent element
 * maximizes the minimum distance to all previously selected elements.
 *
 * Distance metric: per-output, 1.0 if enabled/disabled differs,
 * else |bandwidth_a - bandwidth_b| / max_bandwidth. Sum across outputs.
 */
void spaceFillSort(std::vector<CandidateConfig> &candidates,
                   const std::vector<std::vector<OutputModeOption>> &perOutputOptions);

/**
 * Compute distance between two candidate configurations.
 * Exposed for testing.
 */
double candidateDistance(const CandidateConfig &a, const CandidateConfig &b,
                        const std::vector<std::vector<OutputModeOption>> &perOutputOptions);

} // namespace OutputConfigSearch
} // namespace KWin
