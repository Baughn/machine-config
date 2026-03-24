/*
    KWin - the KDE window manager
    This file is part of the KDE project.

    SPDX-License-Identifier: GPL-2.0-or-later
*/

#include "outputconfigsearch.h"
#include "core/backendoutput.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <map>
#include <ranges>

namespace KWin
{

uint64_t OutputModeOption::bandwidth() const
{
    if (!mode) {
        return 0;
    }
    return uint64_t(mode->size().width()) * mode->size().height() * mode->refreshRate();
}

namespace OutputConfigSearch
{

std::vector<OutputModeOption> pruneModes(BackendOutput *output, int totalOutputCount)
{
    std::vector<OutputModeOption> result;

    const auto modes = output->modes();

    // Filter to non-broken modes
    auto validModes = modes | std::ranges::views::filter([](const auto &mode) {
        return !(mode->flags() & OutputMode::Flag::Generated)
            && !(mode->flags() & OutputMode::Flag::Removed);
    });

    // Group by resolution
    struct ModesBySize
    {
        std::shared_ptr<OutputMode> highestRefresh;
        std::shared_ptr<OutputMode> lowest110Hz; // lowest >= 110000 mHz
        std::shared_ptr<OutputMode> lowest50Hz; // lowest >= 50000 mHz
        std::shared_ptr<OutputMode> preferred; // if any preferred mode at this resolution
    };
    std::map<std::pair<int, int>, ModesBySize> bySize;

    for (const auto &mode : validModes) {
        auto key = std::make_pair(mode->size().width(), mode->size().height());
        auto &entry = bySize[key];

        const uint32_t rate = mode->refreshRate();

        // Highest refresh
        if (!entry.highestRefresh || rate > entry.highestRefresh->refreshRate()) {
            entry.highestRefresh = mode;
        }
        // Lowest >= 110Hz
        if (rate >= 110000) {
            if (!entry.lowest110Hz || rate < entry.lowest110Hz->refreshRate()) {
                entry.lowest110Hz = mode;
            }
        }
        // Lowest >= 50Hz
        if (rate >= 50000) {
            if (!entry.lowest50Hz || rate < entry.lowest50Hz->refreshRate()) {
                entry.lowest50Hz = mode;
            }
        }
        // Preferred
        if (mode->flags() & OutputMode::Flag::Preferred) {
            if (!entry.preferred || rate > entry.preferred->refreshRate()) {
                entry.preferred = mode;
            }
        }
    }

    // Collect unique modes and create options (with native bpc)
    auto addIfUnique = [&result](const std::shared_ptr<OutputMode> &mode, uint32_t bpc) {
        if (!mode) {
            return;
        }
        // Deduplicate by (size, refreshRate, bpc)
        for (const auto &existing : result) {
            if (existing.mode && existing.mode->size() == mode->size()
                && existing.mode->refreshRate() == mode->refreshRate()
                && existing.maxBpc == bpc) {
                return;
            }
        }
        result.push_back(OutputModeOption{mode, bpc});
    };

    for (const auto &[size, modes] : bySize) {
        // Native bpc variants (maxBpc = 0 means automatic)
        addIfUnique(modes.highestRefresh, 0);
        addIfUnique(modes.lowest110Hz, 0);
        addIfUnique(modes.lowest50Hz, 0);
        addIfUnique(modes.preferred, 0);

        // 8bpc variants for bandwidth reduction
        addIfUnique(modes.highestRefresh, 8);
        addIfUnique(modes.lowest110Hz, 8);
        addIfUnique(modes.lowest50Hz, 8);
        addIfUnique(modes.preferred, 8);
    }

    // If no valid modes were found, fall back to all modes
    if (result.empty()) {
        for (const auto &mode : modes) {
            if (!(mode->flags() & OutputMode::Flag::Removed)) {
                addIfUnique(mode, 0);
                addIfUnique(mode, 8);
            }
        }
    }

    // Add "disabled" option if there are multiple outputs
    if (totalOutputCount > 1) {
        result.push_back(OutputModeOption{nullptr, 0});
    }

    return result;
}

std::vector<CandidateConfig> enumerateCandidates(const std::vector<std::vector<OutputModeOption>> &perOutputOptions)
{
    const int numOutputs = perOutputOptions.size();
    if (numOutputs == 0) {
        return {};
    }

    // Calculate total number of combinations
    size_t total = 1;
    for (const auto &options : perOutputOptions) {
        total *= options.size();
        // Safety limit to prevent memory explosion
        if (total > 100000) {
            total = 100000;
            break;
        }
    }

    std::vector<CandidateConfig> result;
    result.reserve(total);

    // Generate cross-product using odometer-style iteration
    std::vector<int> indices(numOutputs, 0);

    while (true) {
        // Check: at least one output must be enabled
        bool anyEnabled = false;
        for (int i = 0; i < numOutputs; i++) {
            if (perOutputOptions[i][indices[i]].mode != nullptr) {
                anyEnabled = true;
                break;
            }
        }

        if (anyEnabled) {
            CandidateConfig candidate;
            candidate.modeChoices = indices;
            result.push_back(std::move(candidate));

            if (result.size() >= 100000) {
                break;
            }
        }

        // Increment odometer
        int pos = numOutputs - 1;
        while (pos >= 0) {
            indices[pos]++;
            if (indices[pos] < static_cast<int>(perOutputOptions[pos].size())) {
                break;
            }
            indices[pos] = 0;
            pos--;
        }
        if (pos < 0) {
            break; // All combinations exhausted
        }
    }

    return result;
}

void scoreCandidates(std::vector<CandidateConfig> &candidates,
                     const std::vector<std::vector<OutputModeOption>> &perOutputOptions)
{
    for (auto &candidate : candidates) {
        double score = 0;
        for (size_t i = 0; i < candidate.modeChoices.size(); i++) {
            const auto &option = perOutputOptions[i][candidate.modeChoices[i]];
            if (option.mode) {
                // Strongly prefer having the output enabled
                score += 10000;
                // Prefer higher resolution
                const double pixels = option.mode->size().width() * option.mode->size().height();
                score += pixels / 1000.0;
                // Prefer higher refresh rate
                score += option.mode->refreshRate() / 100.0;
                // Small penalty for reduced bit depth
                if (option.maxBpc == 8) {
                    score -= 50;
                }
            }
        }
        candidate.score = score;
    }
}

static double computeMaxBandwidth(const std::vector<std::vector<OutputModeOption>> &perOutputOptions)
{
    double maxBw = 0;
    for (const auto &options : perOutputOptions) {
        for (const auto &opt : options) {
            const double bw = opt.bandwidth();
            if (bw > maxBw) {
                maxBw = bw;
            }
        }
    }
    return maxBw > 0 ? maxBw : 1.0;
}

static double candidateDistanceImpl(const CandidateConfig &a, const CandidateConfig &b,
                                    const std::vector<std::vector<OutputModeOption>> &perOutputOptions,
                                    double maxBandwidth)
{
    double distance = 0;
    for (size_t i = 0; i < a.modeChoices.size(); i++) {
        const auto &optA = perOutputOptions[i][a.modeChoices[i]];
        const auto &optB = perOutputOptions[i][b.modeChoices[i]];

        const bool enabledA = optA.mode != nullptr;
        const bool enabledB = optB.mode != nullptr;

        if (enabledA != enabledB) {
            distance += 1.0;
        } else if (enabledA && enabledB) {
            const double bwA = optA.bandwidth();
            const double bwB = optB.bandwidth();
            distance += std::abs(bwA - bwB) / maxBandwidth;
            if (optA.maxBpc != optB.maxBpc) {
                distance += 0.2;
            }
        }
    }
    return distance;
}

double candidateDistance(const CandidateConfig &a, const CandidateConfig &b,
                        const std::vector<std::vector<OutputModeOption>> &perOutputOptions)
{
    return candidateDistanceImpl(a, b, perOutputOptions, computeMaxBandwidth(perOutputOptions));
}

void spaceFillSort(std::vector<CandidateConfig> &candidates,
                   const std::vector<std::vector<OutputModeOption>> &perOutputOptions)
{
    if (candidates.size() <= 1) {
        return;
    }

    // Step 1: Find the highest-scored candidate as the first pick
    auto bestIt = std::max_element(candidates.begin(), candidates.end(),
                                   [](const auto &a, const auto &b) { return a.score < b.score; });

    // Swap it to front
    if (bestIt != candidates.begin()) {
        std::iter_swap(candidates.begin(), bestIt);
    }

    // Step 2: Greedy farthest-point sampling
    const double maxBandwidth = computeMaxBandwidth(perOutputOptions);
    const size_t n = candidates.size();
    std::vector<double> minDistToSelected(n, std::numeric_limits<double>::max());

    for (size_t selected = 1; selected < n; selected++) {
        const auto &lastSelected = candidates[selected - 1];
        for (size_t i = selected; i < n; i++) {
            double d = candidateDistanceImpl(lastSelected, candidates[i], perOutputOptions, maxBandwidth);
            minDistToSelected[i] = std::min(minDistToSelected[i], d);
        }

        // Pick the candidate with maximum min-distance
        size_t bestIdx = selected;
        double bestDist = -1;
        for (size_t i = selected; i < n; i++) {
            if (minDistToSelected[i] > bestDist) {
                bestDist = minDistToSelected[i];
                bestIdx = i;
            }
        }

        // Swap it into position
        if (bestIdx != selected) {
            std::swap(candidates[selected], candidates[bestIdx]);
            std::swap(minDistToSelected[selected], minDistToSelected[bestIdx]);
        }
    }
}

} // namespace OutputConfigSearch
} // namespace KWin
